//! # Submission
//!
//! End-to-end submission test against a local Stalwart (server A: IMAP on
//! :143, SMTP on :2525), spawned via tests/stalwart2.sh. Ignored by default.
//!
//! The unit tests drive `send_one` against an in-process sink, so the SMTP
//! dialogue, the envelope and the acknowledgement releasing the body's pin
//! are proven here. The recipient is the account itself, so one server
//! closes the loop: out through SMTP, back in through IMAP.
//!
//! Start the servers and run with:
//! ```sh
//! ./tests/stalwart2.sh
//! cargo test --test submit -- --ignored
//! ```

mod common;

use std::{
    fs,
    path::Path,
    process::Command,
    thread::sleep,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use io_pimdir::{
    client::{
        producer::{PimdirActionStatus, PimdirProducer},
        reader::PimdirReader,
    },
    collection::PimdirCollectionId,
    intent::PimdirSubmit,
};

use crate::common::{json_report, json_result, pending_actions, store_items, summary_text};

const IMAP_ROOT: &str = "imap://127.0.0.1:143";
const SMTP: &str = "smtp://127.0.0.1:2525";
const USER: &str = "test@pimalaya.org";
const PASS: &str = "P!malaya-test-2026";
/// A single token needing no quoting, distinct per run.
///
/// The server keeps what earlier runs delivered, so a constant marker would
/// let one of those pass this run's search with nothing sent.
fn marker() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after the epoch")
        .as_nanos();

    format!("SUBMITMARKER{nanos}")
}

fn message(marker: &str) -> Vec<u8> {
    format!(
        "Message-ID: <{marker}@pimalaya.org>\r\n\
         From: {USER}\r\n\
         To: {USER}\r\n\
         Subject: neverest submission {marker}\r\n\
         Date: Tue, 25 Aug 2026 10:00:00 +0000\r\n\
         \r\n\
         {marker}\r\n",
    )
    .into_bytes()
}

/// A queued `submit` leaves through SMTP and comes back through IMAP.
///
/// A first sync makes the store a producer only appends to; the body is
/// staged before the row naming it, as a frontend does, and the acknowledged
/// row leaves the queue, applied, releasing the body's pin.
#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :143/:2525 and --ignored"]
fn a_queued_submit_intent_leaves_through_smtp_and_comes_back_through_imap() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();

    fs::write(
        &config,
        format!(
            "[accounts.submit]\n\
             imap.server = \"{IMAP_ROOT}\"\n\
             imap.starttls = false\n\
             imap.sasl.plain.username = \"{USER}\"\n\
             imap.sasl.plain.password.raw = \"{PASS}\"\n\
             smtp.server = \"{SMTP}\"\n",
        ),
    )
    .unwrap();

    neverest(&["init", "-a", "submit"], &config, &state);
    neverest(&["sync", "-a", "submit"], &config, &state);

    let store = state.join("neverest");
    let marker = marker();
    let body = message(&marker);
    let mut producer = PimdirProducer::open(&store, "neverest-tests").expect("open producer");
    let submit = PimdirSubmit {
        source: String::from("submit/imap"),
        from: String::from(USER),
        rcpts: vec![String::from(USER)],
        subject: Some(format!("neverest submission {marker}")),
        copy: None,
    };

    let id = producer
        .enqueue_with_body("submit/imap/INBOX", &body, |object| {
            submit.to_action(object).unwrap()
        })
        .expect("enqueue the intent")
        .handle
        .id;
    drop(producer);

    let queued = pending_actions(&store);
    assert!(
        queued.iter().any(|row| row.id == id),
        "the intent is queued before the run; queue held:\n{queued:#?}",
    );

    let report = neverest(&["sync", "-a", "submit", "--json"], &config, &state);
    let submitted = &json_report(&report)["submitted"];
    assert!(
        submitted.as_array().is_some_and(|s| s.len() == 1) && submitted[0]["error"].is_null(),
        "the run reported the submission; report was:\n{report}",
    );
    assert!(
        !report.contains("\"parked\":true"),
        "the intent was not parked; report was:\n{report}",
    );

    let queued = pending_actions(&store);
    assert!(
        !queued.iter().any(|row| row.id == id),
        "an acknowledged intent leaves the queue; queue held:\n{queued:#?}",
    );

    let producer = PimdirProducer::open(&store, "neverest-tests").expect("open producer");
    let status = producer.action_status(id).expect("read the row's status");
    assert!(
        matches!(status, PimdirActionStatus::Applied { seq: None, .. }),
        "a sent intent reads as applied; it read {status:?}",
    );

    let delivered = wait_for_delivery(&marker).expect("the submitted message reached the server");
    neverest(&["sync", "-a", "submit"], &config, &state);

    let items = store_items(&store, &delivered);
    assert!(
        items
            .iter()
            .any(|item| summary_text(item).contains(&marker)),
        "the submitted message came back into the store; {delivered} held:\n{items:#?}",
    );
}

/// `check` opens the SMTP channel a source declares.
///
/// With the sources still answering, a closed SMTP port fails the check
/// alone; the error lands on either stream depending on the printer.
#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :143/:2525 and --ignored"]
fn check_opens_the_smtp_channel_a_source_declares() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let state = tmp.path().join("state");
    let config = tmp.path().join("config.toml");
    fs::create_dir_all(&state).unwrap();
    let write = |smtp: &str| {
        fs::write(
            &config,
            format!(
                "[accounts.submit]\n\
                 imap.server = \"{IMAP_ROOT}\"\n\
                 imap.starttls = false\n\
                 imap.sasl.plain.username = \"{USER}\"\n\
                 imap.sasl.plain.password.raw = \"{PASS}\"\n\
                 smtp.server = \"{smtp}\"\n",
            ),
        )
        .unwrap();
    };

    write(SMTP);
    let checked = neverest(&["--json", "check", "-a", "submit"], &config, &state);
    assert_eq!(
        json_result(&checked)["accounts"][0]["sources"][0]["smtp"]["ok"],
        true,
        "{checked}"
    );

    write("smtp://127.0.0.1:1");
    let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy(), "check", "-a", "submit"])
        .env("XDG_STATE_HOME", &state)
        .output()
        .expect("spawn neverest");
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "{printed}");
    assert!(printed.contains("Check the SMTP channel"), "{printed}");
}

/// `check` lists the collections with the roles the server states.
///
/// Stalwart creates its default folders with SPECIAL-USE roles; a folder
/// the server states nothing about carries none.
#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :143 and --ignored"]
fn check_lists_collections_with_the_roles_the_server_states() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let state = tmp.path().join("state");
    let config = tmp.path().join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(
        &config,
        format!(
            "[accounts.roles]\n\
             imap.server = \"{IMAP_ROOT}\"\n\
             imap.starttls = false\n\
             imap.sasl.plain.username = \"{USER}\"\n\
             imap.sasl.plain.password.raw = \"{PASS}\"\n",
        ),
    )
    .unwrap();

    let checked = neverest(&["--json", "check", "-a", "roles"], &config, &state);
    let checked_all: serde_json::Value = serde_json::from_str(&checked).expect("one JSON document");
    let report = &checked_all["accounts"][0];
    let collections = report["sources"][0]["collections"]
        .as_array()
        .unwrap_or_else(|| panic!("collections listed: {checked}"));
    let role_of = |role: &str| {
        collections
            .iter()
            .find(|collection| collection["role"] == role)
            .map(|collection| collection["id"].as_str().unwrap_or_default().to_owned())
    };

    assert_eq!(role_of("inbox").as_deref(), Some("INBOX"), "{checked}");
    for role in ["sent", "drafts", "trash", "junk"] {
        assert!(
            role_of(role).is_some(),
            "no {role} folder stated: {checked}"
        );
    }
    assert!(
        collections
            .iter()
            .all(|collection| collection["role"].is_null()
                || ["inbox", "sent", "drafts", "trash", "junk", "all", "archive"]
                    .contains(&collection["role"].as_str().unwrap_or_default())),
        "{checked}"
    );
}

/// `sync --intents-only` sends the queued message and stops.
///
/// Nothing listed, nothing fetched; the sent copy is in the store at once
/// and waits there until the next full sync pushes it.
#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :143/:2525 and --ignored"]
fn intents_only_sends_and_the_next_sync_files_the_copy() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(
        &config,
        format!(
            "[accounts.submit]\n\
             imap.server = \"{IMAP_ROOT}\"\n\
             imap.starttls = false\n\
             imap.sasl.plain.username = \"{USER}\"\n\
             imap.sasl.plain.password.raw = \"{PASS}\"\n\
             smtp.server = \"{SMTP}\"\n",
        ),
    )
    .unwrap();
    neverest(&["init", "-a", "submit"], &config, &state);
    neverest(&["sync", "-a", "submit"], &config, &state);

    let store = state.join("neverest");
    let sent = PimdirReader::open(&store)
        .expect("open reader")
        .list_collections()
        .expect("list collections")
        .into_iter()
        .find(|collection| collection.role.as_deref() == Some("sent"))
        .expect("the server states a Sent folder")
        .id;

    let marker = marker();
    let body = message(&marker);
    let mut producer = PimdirProducer::open(&store, "neverest-tests").expect("open producer");
    let submit = PimdirSubmit {
        source: String::from("submit/imap"),
        from: String::from(USER),
        rcpts: vec![String::from(USER)],
        subject: Some(marker.clone()),
        copy: Some(PimdirCollectionId(sent.clone())),
    };
    let id = producer
        .enqueue_with_body(&sent, &body, |object| submit.to_action(object).unwrap())
        .expect("enqueue the intent")
        .handle
        .id;
    drop(producer);

    let report = neverest(
        &["--json", "sync", "-a", "submit", "--intents-only"],
        &config,
        &state,
    );
    let report = json_report(&report);
    let submitted = &report["submitted"][0];
    assert_eq!(submitted["row"], id, "{report}");
    assert_eq!(submitted["error"], serde_json::Value::Null, "{report}");
    assert_eq!(submitted["unconfirmed"], false, "{report}");
    assert_eq!(report["collectionPatch"], serde_json::json!([]), "{report}");
    assert_eq!(report["itemPatch"], serde_json::json!([]), "{report}");
    assert_eq!(report["drained"][0]["collection"], sent, "{report}");

    wait_for_delivery(&marker).expect("the submitted message reached the server");

    let folder = sent.trim_start_matches("submit/imap/").replace(' ', "%20");
    assert!(
        !search(&folder, &marker),
        "the copy is not on the server yet"
    );
    neverest(&["sync", "-a", "submit"], &config, &state);
    assert!(search(&folder, &marker), "the next sync files the copy");
}

/// Whether the account's `mailbox` (URL-encoded) holds the marker.
fn search(mailbox: &str, marker: &str) -> bool {
    let search = Command::new("curl")
        .args(["-fsS", "--url", &format!("{IMAP_ROOT}/{mailbox}")])
        .args(["--user", &format!("{USER}:{PASS}")])
        .args(["-X", &format!("SEARCH TEXT {marker}")])
        .output()
        .expect("spawn curl search");
    String::from_utf8_lossy(&search.stdout)
        .split_whitespace()
        .any(|token| token.parse::<u32>().is_ok())
}

/// Polls the mailboxes for the marker, answering the store's key of the one.
///
/// The `250` to `DATA` is the server taking the message, not filing it. An
/// unauthenticated sender of the server's own domain reads as spoofing to
/// Stalwart, filed under `Junk Mail`, so both mailboxes are searched.
fn wait_for_delivery(marker: &str) -> Option<String> {
    for _ in 0..30 {
        for (mailbox, path) in [("INBOX", "INBOX"), ("Junk Mail", "Junk%20Mail")] {
            let search = Command::new("curl")
                .args(["-fsS", "--url", &format!("{IMAP_ROOT}/{path}")])
                .args(["--user", &format!("{USER}:{PASS}")])
                .args(["-X", &format!("SEARCH TEXT {marker}")])
                .output()
                .expect("spawn curl search");

            let hits = String::from_utf8_lossy(&search.stdout);
            if hits.split_whitespace().any(|t| t.parse::<u32>().is_ok()) {
                return Some(format!("submit/imap/{mailbox}"));
            }
        }

        sleep(Duration::from_secs(1));
    }

    None
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
