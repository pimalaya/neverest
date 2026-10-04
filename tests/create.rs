//! The `collection-create` intent against a local Stalwart (server A: IMAP
//! on :143, HTTP and DAV on :8080), spawned via `tests/stalwart2.sh`.
//! Ignored by default.
//!
//! What a frontend does to create a folder or a calendar: it queues the
//! intent through io-pimdir's producer, anchored on a collection the store
//! holds, and the next sync creates it on the server before listing, so
//! the same run lists it.
//!
//! Start the servers and run with:
//! ```sh
//! ./tests/stalwart2.sh
//! cargo install --path ../io-pimdir --features cli
//! cargo test --test create -- --ignored --test-threads=1
//! ```

use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use io_pimdir::client::producer::PimdirProducer;
use serde_json::Value;

const IMAP_ROOT: &str = "imap://127.0.0.1:143";
const DAV: &str = "http://127.0.0.1:8080/dav";
const USER: &str = "test@pimalaya.org";
const PASS: &str = "P!malaya-test-2026";

/// A name no earlier run left on the server.
fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after the epoch")
        .as_nanos();
    format!("{prefix}{nanos}")
}

#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :143 and --ignored"]
fn a_queued_folder_and_its_child_are_created_on_the_imap_server() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let state = tmp.path().join("state");
    let config = tmp.path().join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(
        &config,
        format!(
            "[accounts.create]\n\
             imap.server = \"{IMAP_ROOT}\"\n\
             imap.starttls = false\n\
             imap.sasl.plain.username = \"{USER}\"\n\
             imap.sasl.plain.password.raw = \"{PASS}\"\n",
        ),
    )
    .unwrap();

    // NOTE: the first sync makes the store and declares the source, which
    // the producer resolves the performer from.
    neverest(&["init", "-a", "create"], &config, &state);
    sync(&config, &state, "INBOX");

    let folder = unique("Archives");
    let store = state.join("neverest").join("create");
    let mut producer = PimdirProducer::open(&store, "neverest-tests").expect("open producer");
    producer
        .enqueue_collection_create("imap/INBOX", &folder, None, None)
        .expect("queue the folder");

    let report = sync(&config, &state, &folder);
    assert_eq!(intent_errors(&report), vec![Value::Null], "{report}");
    assert!(listed(&config, &state).contains(&folder));

    // The folder now in the store, a child goes under it, joined by the
    // server's hierarchy delimiter.
    producer
        .enqueue_collection_create("imap/INBOX", "2026", Some(&format!("imap/{folder}")), None)
        .expect("queue the child");
    let report = sync(&config, &state, &folder);
    assert_eq!(intent_errors(&report), vec![Value::Null], "{report}");
    let listed = listed(&config, &state);
    assert!(
        listed
            .iter()
            .any(|id| id.starts_with(&folder) && id.ends_with("2026") && *id != folder),
        "{listed:?}"
    );

    // Asked again, a folder the server lists already is success.
    producer
        .enqueue_collection_create("imap/INBOX", &folder, None, None)
        .expect("queue the folder again");
    let report = sync(&config, &state, &folder);
    assert_eq!(intent_errors(&report), vec![Value::Null], "{report}");

    for mailbox in [format!("{folder}/2026"), folder] {
        let _ = Command::new("curl")
            .args(["-sS", "-o", "/dev/null", "-u", &format!("{USER}:{PASS}")])
            .args(["-X", &format!("DELETE \"{mailbox}\"")])
            .arg(format!("{IMAP_ROOT}/"))
            .output();
    }
}

#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :8080 and --ignored"]
fn a_queued_calendar_is_created_on_the_caldav_server() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let state = tmp.path().join("state");
    let config = tmp.path().join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(
        &config,
        format!(
            "[accounts.create]\n\
             caldav.server = \"{DAV}/cal/\"\n\
             caldav.auth.basic.username = \"{USER}\"\n\
             caldav.auth.basic.password.raw = \"{PASS}\"\n",
        ),
    )
    .unwrap();

    neverest(&["init", "-a", "create"], &config, &state);
    sync(&config, &state, "default");

    let name = format!("Team {}", unique(""));
    let store = state.join("neverest").join("create");
    let mut producer = PimdirProducer::open(&store, "neverest-tests").expect("open producer");
    producer
        .enqueue_collection_create("caldav/default", &name, None, None)
        .expect("queue the calendar");

    let report = sync(&config, &state, "default");
    assert_eq!(intent_errors(&report), vec![Value::Null], "{report}");

    let checked = check(&config, &state);
    let created = checked["sources"][0]["collections"]
        .as_array()
        .expect("collections")
        .iter()
        .find(|collection| collection["name"] == name.as_str())
        .unwrap_or_else(|| panic!("{name} not listed: {checked}"))
        .clone();
    let id = created["id"].as_str().unwrap().to_owned();
    assert_eq!(id, name.replace(' ', "-"), "{checked}");

    // NOTE: a calendar does not nest, so a parent parks the row.
    producer
        .enqueue_collection_create("caldav/default", "Sub", Some("caldav/default"), None)
        .expect("queue a child calendar");
    let report = sync(&config, &state, "default");
    let intents: Value = serde_json::from_str(&report).unwrap();
    assert_eq!(intents["intents"][0]["parked"], true, "{report}");

    let _ = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-X", "DELETE", "-u"])
        .arg(format!("{USER}:{PASS}"))
        .arg(format!("{DAV}/cal/test%40pimalaya.org/{id}/"))
        .output();
}

/// Syncs the account narrowed to one collection, and returns the JSON
/// report.
fn sync(config: &Path, state: &Path, collection: &str) -> String {
    neverest(
        &["sync", "-a", "create", "-m", collection, "--json"],
        config,
        state,
    )
}

/// The error of each intent the report names, `null` for one performed.
fn intent_errors(report: &str) -> Vec<Value> {
    let report: Value = serde_json::from_str(report).expect("one JSON document");
    report["intents"]
        .as_array()
        .map(|intents| {
            intents
                .iter()
                .map(|intent| intent.get("error").cloned().unwrap_or(Value::Null))
                .collect()
        })
        .unwrap_or_default()
}

fn check(config: &Path, state: &Path) -> Value {
    let checked = neverest(&["--json", "check", "-a", "create"], config, state);
    serde_json::from_str(&checked).expect("one JSON document")
}

/// The collection ids the server lists.
fn listed(config: &Path, state: &Path) -> Vec<String> {
    check(config, state)["sources"][0]["collections"]
        .as_array()
        .expect("collections")
        .iter()
        .filter_map(|collection| collection["id"].as_str().map(str::to_owned))
        .collect()
}

fn neverest(args: &[&str], config: &Path, state: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(args)
        .env("XDG_STATE_HOME", state)
        .output()
        .expect("spawn neverest");

    assert!(
        output.status.success(),
        "`neverest {}` failed:\n--- stdout ---\n{}\n--- stderr ---\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8_lossy(&output.stdout).into_owned()
}
