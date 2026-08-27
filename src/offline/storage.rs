//! Per-side views over a shared pimdir store.
//!
//! The store persists a [`ReplicaHub`](io_replica::hub::ReplicaHub) per
//! collection — one shared item plus a base per source. The engine's
//! [`ReplicaStorage`] seam (`load` / `lookup_objects` / `write`) is serviced by
//! the [`PimdirSourceStore`] handle itself, one per side (`"left"` /
//! `"right"`); this module adds the two multi-source reads the driver needs on
//! top of that seam:
//!
//! - [`load_side`]: the placements one side's coroutines see (its hub
//!   projection plus its not-yet-linked residual), for the `Meta` upgrade pass;
//! - [`projection_view`]: the cross-side propagation a side owes (a `Created`
//!   copy, a `Dirty` flag change, a `Tombstone` delete), for the report;
//! - [`hydration_targets`]: the one-sided, bodiless items whose body must be
//!   fetched (`Full`) so the other side can receive a copy.
//!
//! [`load_side`] reads through one source handle (so it carries that source's
//! residual); [`projection_view`] and [`hydration_targets`] read the whole hub
//! (both sources' bindings), so they take the source-less handle a side's own
//! handle dereferences to.

use io_pimdir::{PimdirError, PimdirSourceStore, PimdirStore};
use io_replica::{
    client::ReplicaStorage,
    collection::ReplicaCollectionId,
    placement::{ReplicaHandle, ReplicaPlacement},
    storage::ReplicaLoadScope,
};

use crate::offline::source_id;

/// The placements one side's coroutines see for a collection: its hub projection
/// plus this source handle's residual (freshly probed items not yet linked).
/// The handle must be the side's own store (source is fixed at open).
pub fn load_side(
    store: &PimdirSourceStore,
    collection: &str,
) -> Result<Vec<ReplicaPlacement>, PimdirError> {
    Ok(store
        .load(
            &ReplicaCollectionId(collection.to_string()),
            &ReplicaLoadScope::All,
        )?
        .placements)
}

/// The cross-source propagation the source named `source` owes for a
/// collection: the hub projection alone (a `Created` copy in, a `Dirty` flag
/// change, a `Tombstone` delete), without the residual probes. Drives the
/// itemized report. Reads the whole hub, so any source handle serves it.
pub fn projection_view(
    store: &PimdirStore,
    collection: &str,
    source: &str,
) -> Result<Vec<ReplicaPlacement>, PimdirError> {
    let hub = store.load_hub(collection)?;
    Ok(hub.project(
        &ReplicaCollectionId(collection.to_string()),
        &source_id(source),
    ))
}

/// The one-sided, bodiless items whose body must be hydrated (`Full`) so the
/// other source can receive a copy: for each shared item held by exactly one
/// of the pair, with no body yet, whose *other* source may create items, the
/// holding source's name and the item's handle there. Reads the whole hub, so
/// any source handle serves it.
pub fn hydration_targets(
    store: &PimdirStore,
    collection: &str,
    left: (&str, bool),
    right: (&str, bool),
) -> Result<Vec<(String, ReplicaHandle)>, PimdirError> {
    let hub = store.load_hub(collection)?;
    let mut out = Vec::new();
    for item in hub.items.values() {
        if item.deleted || item.object.is_some() || item.sources.len() != 1 {
            continue;
        }
        let (held, binding) = item.sources.iter().next().expect("one source");

        let target_creates = if *held == source_id(left.0) {
            right.1
        } else {
            left.1
        };

        if target_creates {
            out.push((held.0.clone(), binding.handle.clone()));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use io_replica::{
        change::ReplicaWriteOp,
        object::{ReplicaHash, ReplicaObject},
        placement::{
            ReplicaBase, ReplicaFlags, ReplicaLevel, ReplicaLinkId, ReplicaMeta, ReplicaSortKey,
            ReplicaStatus,
        },
    };

    use super::*;

    /// A `Meta`-level, linked placement with a base (the shape a side reports
    /// after its first reconcile).
    fn linked(
        collection: &str,
        handle: &str,
        link: &str,
        object: Option<&str>,
    ) -> ReplicaPlacement {
        ReplicaPlacement {
            collection: ReplicaCollectionId(collection.into()),
            handle: ReplicaHandle(handle.into()),
            link_id: Some(ReplicaLinkId(link.into())),
            object: object.map(|h| ReplicaHash(h.into())),
            level: if object.is_some() {
                ReplicaLevel::Full
            } else {
                ReplicaLevel::Meta
            },
            meta: Some(ReplicaMeta(String::new())),
            sort_key: ReplicaSortKey::default(),
            flags: ReplicaFlags::default(),
            status: ReplicaStatus::Clean,
            conflict_revision: None,
            base: Some(ReplicaBase {
                flags: ReplicaFlags::default(),
                revision: None,
                object: object.map(|h| ReplicaHash(h.into())),
            }),
            origin: None,
            ambiguous_handles: Vec::new(),
        }
    }

    #[test]
    fn a_one_sided_body_projects_as_a_copy_and_is_a_hydration_target() {
        let dir = tempfile::tempdir().unwrap();
        let mut left = PimdirStore::open(dir.path()).unwrap().for_source("left");

        left.write(vec![
            ReplicaWriteOp::StoreObject {
                object: ReplicaObject {
                    hash: ReplicaHash("abcd0000".into()),
                    size: 3,
                },
                body: Some(b"abc".to_vec()),
            },
            ReplicaWriteOp::UpsertPlacement(linked("INBOX", "1", "mid:a", Some("abcd0000"))),
        ])
        .unwrap();

        let right_view = projection_view(&left, "INBOX", "right").unwrap();
        assert_eq!(right_view.len(), 1);
        assert_eq!(right_view[0].status, ReplicaStatus::Created);
        assert_eq!(right_view[0].object, Some(ReplicaHash("abcd0000".into())));
    }

    #[test]
    fn hydration_targets_pick_one_sided_bodiless_items_when_the_far_side_creates() {
        let dir = tempfile::tempdir().unwrap();
        let mut left = PimdirStore::open(dir.path()).unwrap().for_source("left");

        left.write(vec![ReplicaWriteOp::UpsertPlacement(linked(
            "INBOX", "1", "mid:a", None,
        ))])
        .unwrap();

        let targets = hydration_targets(&left, "INBOX", ("left", false), ("right", true)).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].0, "left");
        assert_eq!(targets[0].1.0, "1");

        assert!(
            hydration_targets(&left, "INBOX", ("left", false), ("right", false))
                .unwrap()
                .is_empty()
        );
    }
}
