//! End-to-end submission test against a local Stalwart (server A: IMAP on
//! :143, SMTP on :2525), spawned via `tests/stalwart2.sh`. Ignored by default.
//!
//! Submission is the one path the unit tests can only fake: they drive
//! `send_one` against an in-process sink, so the SMTP dialogue, the envelope
//! the payload carries and the acknowledgement that releases the body's pin
//! were never proven against a server. Here they are:
//!
//!   1. Sync once so the store exists at the owner's schema version.
//!   2. Stage a body in the blob tree and enqueue a `submit` intent naming it,
//!      exactly as a frontend (himalaya) produces one.
//!   3. Sync, and check the report says the intent was submitted and that the
//!      queue row is gone, which is what releases the body's pin.
//!   4. Wait for the server to deliver it back to the same account, sync, and
//!      check the message reached the store through the IMAP side: the send
//!      and the pull are one chain, not two.
//!
//! The recipient is the account itself, so a single server closes the loop
//! without a relay: submission goes out through SMTP and comes back in
//! through IMAP.
//!
//! Start the servers and run with:
//! ```sh
//! ./tests/stalwart2.sh
//! cargo test --test submit -- --ignored
//! ```

use std::{
    fs,
    io::Write,
    path::Path,
    process::Command,
    thread::sleep,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use io_pimdir::{PimdirBlobs, PimdirProducer, codec::PimdirAction};

const IMAP_ROOT: &str = "imap://127.0.0.1:143";
const SMTP: &str = "smtp://127.0.0.1:2525";
const USER: &str = "test@pimalaya.org";
const PASS: &str = "P!malaya-test-2026";
/// The queue action kind neverest defines for a submission.
const SUBMIT: &str = "submit";

/// A single token, so the IMAP `SEARCH TEXT` needs no quoting, and a distinct
/// one per run: the server keeps what earlier runs delivered to it, and a
/// constant marker would let one of those messages pass this run's search
/// without anything having been sent.
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

    // 1. A first sync, which is what creates the store the producer needs:
    //    a producer never creates one, it appends to the owner's.
    neverest(&["init", "-a", "submit"], &config, &state);
    neverest(&["sync", "-a", "submit"], &config, &state);

    // 2. What a frontend does: the body durably in the blob tree first, then
    //    the queue row naming it, so the body is never unreferenced with the
    //    intent already queued.
    let store = state.join("neverest").join("submit");
    let marker = marker();
    let body = message(&marker);
    let mut producer = PimdirProducer::open(&store, "neverest-tests").expect("open producer");
    let hash = producer.hash(&body);
    let blobs = PimdirBlobs::open(&store, producer.hash_algo());
    let mut writer = blobs.writer().expect("blob writer");
    writer.write_all(&body).unwrap();
    let size = writer.commit(&hash).expect("commit body");

    producer
        .enqueue(
            "INBOX",
            &PimdirAction::Unknown {
                kind: SUBMIT.into(),
                payload: format!(
                    "{{\"v\":1,\"object\":\"{}\",\"from\":\"{USER}\",\
                     \"rcpts\":[\"{USER}\"],\"subject\":\"neverest submission {marker}\"}}",
                    hash.0,
                ),
                object_hash: Some(hash.clone()),
            },
            Some(size),
            "2026-08-25T10:00:00Z",
        )
        .expect("enqueue the intent");
    drop(producer);

    let queued = pimdir(&state, &["queue", "list", "--json"]);
    assert!(
        queued.contains(SUBMIT),
        "the intent is queued before the run; queue held:\n{queued}",
    );

    // 3. The run performs it: sent, acknowledged, and no longer queued.
    let report = neverest(&["sync", "-a", "submit", "--json"], &config, &state);
    assert!(
        report.contains("\"submitted\""),
        "the run reported the submission; report was:\n{report}",
    );
    assert!(
        !report.contains("\"parked\":true"),
        "the intent was not parked; report was:\n{report}",
    );

    let queued = pimdir(&state, &["queue", "list", "--json"]);
    assert!(
        !queued.contains(SUBMIT),
        "an acknowledged intent leaves the queue; queue held:\n{queued}",
    );

    // 4. The server delivered it back to the same account, and the IMAP side
    //    pulled it into the store: one chain, submission to replica.
    let delivered = wait_for_delivery(&marker).expect("the submitted message reached the server");
    neverest(&["sync", "-a", "submit"], &config, &state);

    let items = pimdir(&state, &["item", "list", &delivered, "--json"]);
    assert!(
        items.contains(&marker),
        "the submitted message came back into the store; `{delivered}` held:\n{items}",
    );
}

/// Polls the account's mailboxes for the marker until one holds it, and
/// answers which. Delivery is queued: the `250` that acknowledged the `DATA`
/// is the server taking the message, not the server having filed it.
///
/// Which mailbox it lands in is the server's call, not this chain's. Port 25
/// offers no `AUTH`, so the submission arrives unauthenticated with a sender
/// in the server's own domain, and Stalwart's filter reads that as spoofing
/// and files it under `Junk Mail`. The sync collects every mailbox, so the
/// verdict changes where the message is, never whether it arrives.
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
                return Some(mailbox.to_owned());
            }
        }

        sleep(Duration::from_secs(1));
    }

    None
}

/// Runs the `pimdir` operator CLI against the account's store, the way the
/// CardDAV test does: the queue and the retained rows are what the sync
/// report does not print.
fn pimdir(state: &Path, args: &[&str]) -> String {
    let store = state.join("neverest").join("submit");
    let output = Command::new("pimdir")
        .args(["--store", &store.to_string_lossy()])
        .args(args)
        .output()
        .expect("spawn pimdir (cargo install --path ../io-pimdir --features cli)");

    assert!(
        output.status.success(),
        "`pimdir {}` failed:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8_lossy(&output.stdout).into_owned()
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
