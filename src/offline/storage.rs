//! # Per-side store views
//!
//! The store persists a [`PimdirHub`](io_pimdir::hub::PimdirHub) per
//! collection, one shared item plus a base per source, and services the
//! engine's storage seam through the [`PimdirSourceStore`] handle itself.
//!
//! This module adds the multi-source reads the driver needs on top of that
//! seam. [`load_side`] reads through one source handle, so it carries that
//! source's probes, while [`projection_view`] and [`hydration_targets`] read
//! the whole hub, both sources' bindings included.

use io_pimdir::{
    client::{PimdirError, PimdirSourceStore, PimdirStore},
    collection::PimdirCollectionId,
    hub::PimdirSourceId,
    load::PimdirLoadScope,
    placement::{PimdirHandle, PimdirPlacement},
};

use crate::offline::source_id;

/// The placements one side's coroutines see for a collection.
///
/// Its hub projection plus this source's probes (freshly enumerated handles
/// not yet named). The handle must be the side's own store, source fixed at
/// open.
pub fn load_side(
    store: &PimdirSourceStore,
    collection: &str,
) -> Result<Vec<PimdirPlacement>, PimdirError> {
    Ok(store
        .load(
            &PimdirCollectionId(collection.to_string()),
            &PimdirLoadScope::All,
        )?
        .placements)
}

/// The cross-source propagation `source` owes for a collection.
///
/// The hub projection alone (a `Created` copy in, a `Dirty` flag change, a
/// `Tombstone` delete), without the probes. Drives the itemized report, and
/// reads the whole hub, so any source handle serves it.
pub fn projection_view(
    store: &PimdirStore,
    collection: &str,
    source: &str,
) -> Result<Vec<PimdirPlacement>, PimdirError> {
    let hub = store.load_hub(collection)?;
    Ok(hub.project(
        &PimdirCollectionId(collection.to_string()),
        &source_id(source),
    ))
}

/// One endpoint of a pair, as hydration reads it.
pub struct HydrationSide<'a> {
    /// The source id, as the account names it.
    pub name: &'a str,
    /// Whether the sync may create an item on it.
    pub creates: bool,
    /// Whether the sync may replace the body of an item it holds.
    pub updates: bool,
    /// Whether this side is the declared truth, so a difference resolves in
    /// its favour rather than being a divergence.
    pub decides: bool,
}

/// The bodiless items to hydrate (`Full`) for the pair to converge this run.
///
/// An item one side alone holds is a copy the other may be given. An item both
/// hold, changed on exactly one, is an update the other is owed: without the
/// body the push has nothing to send. An item both rewrote is a divergence.
pub fn hydration_targets(
    store: &PimdirStore,
    collection: &str,
    left: HydrationSide<'_>,
    right: HydrationSide<'_>,
) -> Result<Vec<(String, PimdirHandle)>, PimdirError> {
    let hub = store.load_hub(collection)?;
    let left_id = source_id(left.name);
    let mut out = Vec::new();

    for item in hub.items.values() {
        if item.deleted || item.object.is_some() || item.sources.is_empty() {
            continue;
        }

        let other = |held: &PimdirSourceId| match *held == left_id {
            true => &right,
            false => &left,
        };

        if item.sources.len() == 1 {
            let (held, binding) = item.sources.iter().next().expect("one source");
            if other(held).creates {
                out.push((held.0.clone(), binding.handle.clone()));
            }
            continue;
        }

        // NOTE: a base that carries no body is the mark a pull leaves when
        // the remote changed the content, since recording it drops the stale
        // body from the item and from that source's base together. A base
        // that is absent altogether never agreed with anything.
        let pulled: Vec<_> = item
            .sources
            .iter()
            .filter(|(_, binding)| {
                binding
                    .base
                    .as_ref()
                    .is_some_and(|base| base.object.is_none())
            })
            .collect();

        // NOTE: a difference both sides made is a divergence, which the
        // conflict path merges or parks, unless an authority is declared:
        // then the deciding side overwrites the other.
        let changed = match pulled.as_slice() {
            [(source, binding)] => Some((*source, *binding)),
            _ => pulled
                .iter()
                .find(|(source, _)| {
                    let side = match **source == left_id {
                        true => &left,
                        false => &right,
                    };
                    side.decides
                })
                .map(|(source, binding)| (*source, *binding)),
        };

        let Some((changed, binding)) = changed else {
            continue;
        };

        if other(changed).updates {
            out.push((changed.0.clone(), binding.handle.clone()));
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use io_pimdir::{
        change::PimdirWriteOp,
        object::{PimdirHash, PimdirObject},
        placement::{
            PimdirBase, PimdirFlags, PimdirLevel, PimdirLinkId, PimdirSortKey, PimdirStatus,
        },
        summary::{PimdirSummary, mail::PimdirMailSummary},
    };

    use super::*;

    /// A `Meta`-level linked placement with a base, as after a first reconcile.
    fn linked(collection: &str, handle: &str, link: &str, object: Option<&str>) -> PimdirPlacement {
        PimdirPlacement {
            collection: PimdirCollectionId(collection.into()),
            handle: PimdirHandle(handle.into()),
            link_id: Some(PimdirLinkId(link.into())),
            object: object.map(|h| PimdirHash(h.into())),
            level: if object.is_some() {
                PimdirLevel::Full
            } else {
                PimdirLevel::Meta
            },
            summary: Some(PimdirSummary::Mail(PimdirMailSummary::default())),
            sort_key: PimdirSortKey::default(),
            flags: PimdirFlags::default(),
            status: PimdirStatus::Clean,
            conflict_revision: None,
            conflict_object: None,
            base: Some(PimdirBase {
                flags: PimdirFlags::default(),
                revision: None,
                object: object.map(|h| PimdirHash(h.into())),
            }),
            origin: None,
        }
    }

    #[test]
    fn a_one_sided_body_projects_as_a_copy_and_is_a_hydration_target() {
        let dir = tempfile::tempdir().unwrap();
        let mut left = PimdirStore::open(dir.path()).unwrap().for_source("left");

        left.write(vec![
            PimdirWriteOp::StoreObject {
                object: PimdirObject {
                    hash: PimdirHash("abcd0000".into()),
                    size: 3,
                },
                body: Some(b"abc".to_vec()),
            },
            PimdirWriteOp::UpsertPlacement(linked("INBOX", "1", "mid:a", Some("abcd0000"))),
        ])
        .unwrap();

        let right_view = projection_view(&left, "INBOX", "right").unwrap();
        assert_eq!(right_view.len(), 1);
        assert_eq!(right_view[0].status, PimdirStatus::Created);
        assert_eq!(right_view[0].object, Some(PimdirHash("abcd0000".into())));
    }

    #[test]
    fn hydration_targets_pick_one_sided_bodiless_items_when_the_far_side_creates() {
        let dir = tempfile::tempdir().unwrap();
        let mut left = PimdirStore::open(dir.path()).unwrap().for_source("left");

        left.write(vec![PimdirWriteOp::UpsertPlacement(linked(
            "INBOX", "1", "mid:a", None,
        ))])
        .unwrap();

        let targets =
            hydration_targets(&left, "INBOX", side("left", false), side("right", true)).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].0, "left");
        assert_eq!(targets[0].1.0, "1");

        assert!(
            hydration_targets(&left, "INBOX", side("left", false), side("right", false))
                .unwrap()
                .is_empty()
        );
    }

    /// A side that may take neither a copy nor an update, for the tests that
    /// vary one permission at a time.
    fn side(name: &str, creates: bool) -> HydrationSide<'_> {
        HydrationSide {
            name,
            creates,
            updates: creates,
            decides: false,
        }
    }
}
