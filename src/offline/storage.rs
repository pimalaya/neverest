//! # Per-side store views
//!
//! The store persists a [`ReplicaHub`](io_replica::hub::ReplicaHub) per
//! collection, one shared item plus a base per source, and services the
//! [`ReplicaStorage`] seam through the [`PimdirSourceStore`] handle itself.
//!
//! This module adds the multi-source reads the driver needs on top of that
//! seam. [`load_side`] reads through one source handle, so it carries that
//! source's residual, while [`projection_view`] and [`hydration_targets`]
//! read the whole hub, both sources' bindings included.
//!
//! [`HeldStore`] narrows the seam back to what a source holds, for the one
//! coroutine that reads a placement as a claim on an identity.

use std::collections::{BTreeMap, HashSet};

use io_pimdir::{PimdirError, PimdirSourceStore, PimdirStore};
use io_replica::{
    change::ReplicaWriteOp,
    client::ReplicaStorage,
    collection::ReplicaCollectionId,
    object::ReplicaHash,
    placement::{ReplicaHandle, ReplicaLinkId, ReplicaPlacement},
    storage::{ReplicaLoadScope, ReplicaLoaded},
};

use crate::offline::source_id;

/// The placements one side's coroutines see for a collection.
///
/// Its hub projection plus this handle's residual (freshly probed items not yet
/// linked). The handle must be the side's own store, source fixed at open.
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

/// The cross-source propagation `source` owes for a collection.
///
/// The hub projection alone (a `Created` copy in, a `Dirty` flag change, a
/// `Tombstone` delete), without the residual probes. Drives the itemized
/// report, and reads the whole hub, so any source handle serves it.
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

/// A source's store as an upgrade must read it: what that source holds, without
/// the copies the hub is offering it.
///
/// A projection answers a source with the items it holds plus the ones a
/// sibling holds and it does not, so that the merge derives the append. An
/// upgrade asks a different question, who already holds this identity here,
/// and a copy on offer is not a holder.
///
/// Left in, the second endpoint of an account reads its own card as a copy of
/// the first endpoint's and is minted a key of its own (pimdir SPEC §9), which
/// strands one identity as two items neither server will take.
pub struct HeldStore<'a> {
    store: &'a mut PimdirSourceStore,
    /// The link ids this store's source is bound to in the collection.
    held: HashSet<ReplicaLinkId>,
}

impl<'a> HeldStore<'a> {
    /// Wraps `store`, reading which identities its source holds in
    /// `collection`.
    ///
    /// Read once: an upgrade writes only after its last load, so nothing it
    /// does can add a holder behind the wrap.
    pub fn open(store: &'a mut PimdirSourceStore, collection: &str) -> Result<Self, PimdirError> {
        let source = source_id(store.source());
        let held = store
            .load_hub(collection)?
            .items
            .iter()
            .filter(|(_, item)| item.sources.contains_key(&source))
            .map(|(link, _)| link.clone())
            .collect();

        Ok(Self { store, held })
    }
}

impl ReplicaStorage for HeldStore<'_> {
    type Error = PimdirError;

    fn load(
        &self,
        collection: &ReplicaCollectionId,
        scope: &ReplicaLoadScope,
    ) -> Result<ReplicaLoaded, Self::Error> {
        let mut loaded = self.store.load(collection, scope)?;
        // NOTE: an unlinked placement claims no identity yet, so it stays: it
        // is the freshly probed row the upgrade was called for.
        loaded
            .placements
            .retain(|placement| match &placement.link_id {
                Some(link) => self.held.contains(link),
                None => true,
            });

        Ok(loaded)
    }

    fn lookup_objects(
        &self,
        links: &[ReplicaLinkId],
    ) -> Result<BTreeMap<ReplicaLinkId, ReplicaHash>, Self::Error> {
        self.store.lookup_objects(links)
    }

    fn write(&mut self, ops: Vec<ReplicaWriteOp>) -> Result<(), Self::Error> {
        self.store.write(ops)
    }
}

/// The one-sided, bodiless items whose body must be hydrated (`Full`).
///
/// For each shared item held by exactly one of the pair, with no body yet,
/// whose other source may create items: the holding source's name and the
/// item's handle there. Reads the whole hub, so any source handle serves it.
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

    /// A `Meta`-level linked placement with a base, as after a first reconcile.
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
            conflict_object: None,
            base: Some(ReplicaBase {
                flags: ReplicaFlags::default(),
                revision: None,
                object: object.map(|h| ReplicaHash(h.into())),
            }),
            origin: None,
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

    /// The copy one source is offered is another source's holding, and reading
    /// it as this one's is what mints a duplicate key for the item the second
    /// endpoint of an account already has.
    #[test]
    fn a_copy_on_offer_is_not_read_as_a_holding_of_the_side_it_is_offered_to() {
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

        let mut right = PimdirStore::open(dir.path()).unwrap().for_source("right");
        assert_eq!(
            load_side(&right, "INBOX").unwrap().len(),
            1,
            "the projection offers right the copy left holds",
        );

        let held = HeldStore::open(&mut right, "INBOX").unwrap();
        let loaded = held
            .load(&ReplicaCollectionId("INBOX".into()), &ReplicaLoadScope::All)
            .unwrap();
        assert!(
            loaded.placements.is_empty(),
            "and an upgrade reads right as holding nothing; it read {:?}",
            loaded.placements,
        );

        let held = HeldStore::open(&mut left, "INBOX").unwrap();
        let loaded = held
            .load(&ReplicaCollectionId("INBOX".into()), &ReplicaLoadScope::All)
            .unwrap();
        assert_eq!(loaded.placements.len(), 1, "while left still holds its own",);
    }
}
