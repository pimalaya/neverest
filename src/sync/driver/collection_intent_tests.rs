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
