//! # Collection intents
//!
//! A collection change a frontend queued, performed by a sync against a
//! fake CardDAV server, and what the report says of it.
//!
//! ```sh
//! cargo test --test collection_intents
//! ```

#![cfg(feature = "carddav")]

mod fakedav;

use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

use io_pimdir::{client::producer::PimdirProducer, codec::PimdirAction};
use serde_json::Value;

use fakedav::FakeDav;

/// Writes an account `home` syncing the fake's book into a store under
/// `home`, and returns the configuration path.
fn local(home: &Path, dav: &FakeDav) -> String {
    let store = home.join("store").display().to_string();
    let toml = format!(
        "[accounts.home]\n\
         store.root = {store:?}\n\
         carddav.home = {dav:?}\n\
         carddav.auth.basic.username = \"u\"\n\
         carddav.auth.basic.password.raw = \"p\"\n",
        dav = dav.home,
    );
    let path = home.join("config.toml");
    fs::write(&path, toml).unwrap();
    path.display().to_string()
}

/// Runs `neverest -c <config> sync <args>`, returning its exit status and
/// standard output.
fn sync(home: &Path, config: &str, args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", config, "sync"])
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("XDG_STATE_HOME", home)
        .env_remove("XDG_CONFIG_DIRS")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    (output.status.code().unwrap(), stdout)
}

/// Queues a rename of the book to `name`, as a frontend does.
fn queue_rename(home: &Path, name: &str) {
    let payload = format!(r#"{{"v":1,"source":"home/carddav","name":"{name}"}}"#);
    PimdirProducer::open(home.join("store"), "test-frontend")
        .unwrap()
        .enqueue(
            "home/carddav/book",
            &PimdirAction::Unknown {
                kind: String::from("collection-rename"),
                payload,
                object_hash: None,
            },
            None,
        )
        .unwrap();
}

/// A rename the run performed is a collection hunk, in the JSON and the
/// text: the run wrote to the server, so it is not "already in sync".
///
/// It was listed among the intents alone, the run then saying "already in
/// sync" with `collectionPatch` empty.
#[test]
fn a_performed_rename_is_a_collection_hunk() {
    let home = tempfile::tempdir().unwrap();
    let dav = FakeDav::start();
    let config = local(home.path(), &dav);
    let (code, out) = sync(home.path(), &config, &["--init"]);
    assert_eq!(code, 0, "{out}");

    queue_rename(home.path(), "Renamed");
    let (code, out) = sync(home.path(), &config, &["--json"]);
    assert_eq!(code, 0, "{out}");
    let result: Value = serde_json::from_str(out.lines().last().unwrap()).unwrap();
    let report = &result["accounts"][0]["report"];
    let patch = report["collectionPatch"].as_array().unwrap();
    assert_eq!(patch.len(), 1, "{report}");
    assert_eq!(patch[0]["kind"], "rename", "{report}");
    assert_eq!(patch[0]["collection"], "book", "{report}");
    assert_eq!(patch[0]["to"], "Renamed", "{report}");

    queue_rename(home.path(), "Again");
    let (code, out) = sync(home.path(), &config, &[]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(" - rename collection book to Again on "),
        "{out}"
    );
    assert!(!out.contains("already in sync"), "{out}");
}
