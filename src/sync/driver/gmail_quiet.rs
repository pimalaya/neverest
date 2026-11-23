//! # Gmail quiet labels
//!
//! A run reads the mailbox history once; a settled label it names no change
//! for is left out of phase 1, its checkpoint moved where the mailbox stands.

use io_pimdir::{
    collection::{PimdirCheckpoint, PimdirCollectionId, PimdirScope},
    load::PimdirLoadSelection,
};

use super::{
    PimdirBase, PimdirFlags, PimdirHandle, PimdirLevel, PimdirLinkId, PimdirPlacement,
    PimdirSortKey, PimdirSourceStore, PimdirStatus, PimdirWriteOp, gmail_bodies::connection,
    open_store, quiet_collections,
};
use crate::scripted::ok;

const INBOX: &str = "gmail/INBOX";
const WORK: &str = "gmail/Work";

/// A store binding `m1` in the inbox and `m2` under Work, each label's round
/// closed at history id 500.
fn settled(dir: &std::path::Path) -> PimdirSourceStore {
    let mut store = open_store(dir, "gmail", "gmail").unwrap();
    let mut writes = Vec::new();
    for (collection, id) in [(INBOX, "m1"), (WORK, "m2")] {
        store
            .ensure_collection(collection, "message/rfc822")
            .unwrap();
        let collection = PimdirCollectionId(collection.into());
        writes.extend([
            PimdirWriteOp::OpenRound {
                collection: collection.clone(),
                scope: PimdirScope::unbounded(),
                band: false,
            },
            PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: collection.clone(),
                handle: PimdirHandle(id.into()),
                link_id: Some(PimdirLinkId(format!("{id}@x"))),
                object: None,
                level: PimdirLevel::Meta,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Clean,
                conflict_revision: None,
                conflict_object: None,
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: None,
                    object: None,
                }),
                origin: None,
                server_object_id: None,
            }),
            PimdirWriteOp::CloseRound {
                collection,
                coverage: PimdirScope::unbounded(),
                checkpoint: Some(PimdirCheckpoint(b"500".to_vec())),
            },
        ]);
    }
    store.write(writes).unwrap();
    assert_eq!(store.settled_collections().unwrap().len(), 2);
    store
}

fn checkpoint(store: &PimdirSourceStore, collection: &str) -> Vec<u8> {
    store
        .load(
            &PimdirCollectionId(collection.into()),
            &PimdirLoadSelection::Handles(Vec::new()),
        )
        .unwrap()
        .checkpoint
        .unwrap()
        .0
}

/// A history naming nothing leaves both labels out with one read, their
/// checkpoints moved to the history id the mailbox stands at.
#[test]
fn a_label_its_history_names_nothing_for_is_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = settled(dir.path());
    let (mut ctx, api, _batch) = connection();
    api.answer(ok(serde_json::json!({ "history": [], "historyId": "520" })));

    let collections = vec![String::from(INBOX), String::from(WORK)];
    let quiet = quiet_collections(&mut ctx, &mut store, &collections).unwrap();

    assert_eq!(quiet, collections);
    let lines = api.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("/history?"), "{lines:?}");
    assert!(lines[0].contains("startHistoryId=500"), "{lines:?}");
    assert_eq!(checkpoint(&store, INBOX), b"520");
    assert_eq!(checkpoint(&store, WORK), b"520");
}

/// A star on Work's message keeps Work listed and leaves the inbox out; the
/// history names it under no inbox label and the inbox binds it not.
#[test]
fn a_label_its_history_names_is_listed() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = settled(dir.path());
    let (mut ctx, api, _batch) = connection();
    api.answer(ok(serde_json::json!({
        "history": [{
            "id": "510",
            "labelsAdded": [{
                "message": { "id": "m2", "labelIds": ["Label_1", "STARRED"] },
                "labelIds": ["STARRED"]
            }]
        }],
        "historyId": "520"
    })));

    let collections = vec![String::from(INBOX), String::from(WORK)];
    let quiet = quiet_collections(&mut ctx, &mut store, &collections).unwrap();

    assert_eq!(quiet, [INBOX]);
    assert_eq!(checkpoint(&store, INBOX), b"520");
    assert_eq!(checkpoint(&store, WORK), b"500");
}
