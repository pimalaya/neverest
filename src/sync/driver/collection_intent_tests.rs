//! # Collection intent tests
//!
//! A collection rename the server performed, carried through the store.

use io_pimdir::client::reader::PimdirReader;

use super::*;

/// A rename the server performed under a new id carries the collection
/// and every one under it in the store; one keeping its id is renamed.
///
/// Siblings are left alone, each moved one named as its next listing
/// names it.
#[test]
fn a_performed_rename_carries_the_collections_under_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = open_store(dir.path(), "imap", "live").unwrap();
    for collection in ["imap/A", "imap/A/B", "imap/AB", "imap/C"] {
        store
            .ensure_collection(collection, "message/rfc822")
            .unwrap();
    }

    let renamed = Renamed {
        id: "Z".into(),
        name: "Z".into(),
        delimiter: Some('/'),
    };
    follow_rename(&mut store, "imap", "imap/A", &renamed);

    let collections = PimdirReader::open(dir.path())
        .unwrap()
        .list_collections()
        .unwrap();
    let mut names: Vec<(String, String)> = collections
        .into_iter()
        .map(|collection| (collection.id, collection.name))
        .collect();
    names.sort();
    let ids: Vec<&str> = names.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["imap/AB", "imap/C", "imap/Z", "imap/Z/B"]);
    let name = |id: &str| names.iter().find(|(held, _)| held == id).unwrap().1.clone();
    assert_eq!(name("imap/Z"), "Z");
    assert_eq!(name("imap/Z/B"), "Z/B");

    let kept = Renamed {
        id: "C".into(),
        name: "Team".into(),
        delimiter: None,
    };
    follow_rename(&mut store, "imap", "imap/C", &kept);
    let reader = PimdirReader::open(dir.path()).unwrap();
    let c = reader
        .list_collections()
        .unwrap()
        .into_iter()
        .find(|collection| collection.id == "imap/C")
        .unwrap();
    assert_eq!(c.name, "Team");
}

/// A send queued beside a rename of the collection its copy goes to files
/// the copy under the collection's new id, drained in the same run.
///
/// The copy once named the old id, gone from the store: the sent message
/// could not be acknowledged, and the next run parked it unconfirmed.
#[cfg(any(feature = "smtp", feature = "msgraph", feature = "gmail"))]
#[test]
fn a_send_files_its_copy_in_the_collection_renamed_beside_it() {
    use io_pimdir::client::producer::PimdirProducer;

    let dir = tempfile::tempdir().unwrap();
    let mut store = open_store(dir.path(), "imap", "live").unwrap();
    for collection in ["imap/INBOX", "imap/sep"] {
        store
            .ensure_collection(collection, "message/rfc822")
            .unwrap();
    }
    let object = store
        .blob_stage()
        .stage_body(b"Subject: hi\r\n\r\nhello")
        .unwrap();
    PimdirProducer::open(dir.path(), "test-frontend")
        .unwrap()
        .enqueue(
            "imap/INBOX",
            &PimdirAction::Unknown {
                kind: submit::SUBMIT.into(),
                payload: r#"{"v":1,"source":"imap","from":"a@x.org","rcpts":["b@y.org"],"copy":"imap/sep"}"#
                    .into(),
                object_hash: Some(object.hash.clone()),
            },
            Some(&object),
        )
        .unwrap();

    let renamed = Renamed {
        id: "sep2".into(),
        name: "sep2".into(),
        delimiter: Some('/'),
    };
    let mut renames = Renames::default();
    renames.extend(follow_rename(&mut store, "imap", "imap/sep", &renamed));
    let intents = submit::pending(&store).unwrap();

    let (acknowledged, copied) = acknowledge_sent(&mut store, &intents[0], false, &renames);

    assert!(acknowledged.unwrap(), "the send is acknowledged");
    assert!(copied);
    let pending = PimdirReader::open(dir.path())
        .unwrap()
        .list_pending_actions()
        .unwrap();
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert_eq!(pending[0].collection, "imap/sep2");
    assert!(matches!(pending[0].action, PimdirAction::Add { .. }));
}
