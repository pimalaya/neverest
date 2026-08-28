//! End-to-end proof that a collection holding one identity twice syncs both
//! copies, against TWO local Stalwart IMAP servers (A :143, B :144) spawned by
//! `tests/stalwart2.sh`. Ignored by default.
//!
//! One collection may hold two messages with the same `Message-ID`, and a copy
//! legitimately carries the identifier of the message it copies. The engine
//! mints a key of its own for the second copy (pimdir SPEC §9) instead of
//! freezing the identity, so what this replays is the write side of that:
//!   1. Seed one copy on A and two on B, then sync. Both copies cross: A ends
//!      up holding the message twice, and the report warns about nothing.
//!   2. Sync again with nothing changed. The run is quiescent, which is what
//!      the freeze could never be: a frozen twin had no row, so every run
//!      reported it as a body still to fetch.
//!   3. Delete one copy on B and sync. Exactly that copy goes on A, and the
//!      other survives, which is what two rows buy over one frozen pair.
//!   4. Drop B's checkpoint (a UIDVALIDITY bump, a server without QRESYNC, a
//!      reset) and sync. The full enumeration must re-append nothing: a bound
//!      handle keeps the key it was given and is never minted a second one.
//!
//! The minting and the keys are upstream (io-replica and io-pimdir, same
//! change id); what is proved here is that they hold end to end through this
//! crate, and that no copy is lost on the way.
//!
//! Seeding and verifying use `curl` rather than a backend of this crate, as
//! the other live tests do.
//!
//! Start the servers and run with:
//! ```sh
//! ./tests/stalwart2.sh
//! cargo test --test duplicates -- --ignored
//! ```

use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use io_pimdir::PimdirStore;
use io_replica::{
    change::ReplicaWriteOp,
    collection::{ReplicaCheckpoint, ReplicaCollectionId},
};

const A: &str = "imap://127.0.0.1:143/INBOX";
const B: &str = "imap://127.0.0.1:144/INBOX";
const CRED: &str = "test@pimalaya.org:P!malaya-test-2026";
const ACCOUNT: &str = "dup";

/// A single-token marker, unique per run so the `SEARCH TEXT` sees only this
/// run's copies (the servers keep what earlier runs seeded) and needs no
/// quoting.
fn marker() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_nanos();
    format!("DUPMARKER{nanos}")
}

/// The one message both sides hold, twice on B. A copy legitimately carries
/// the identifier of the message it copies, which is why this is a duplicate
/// to mirror rather than a fault to blame.
fn message(marker: &str) -> Vec<u8> {
    format!(
        "Message-ID: <{marker}@pimalaya.org>\r\n\
         From: alice@pimalaya.org\r\n\
         To: bob@pimalaya.org\r\n\
         Subject: neverest duplicate identity\r\n\
         Date: Tue, 25 Aug 2026 10:00:00 +0000\r\n\
         \r\n\
         {marker}\r\n",
    )
    .into_bytes()
}

#[test]
#[ignore = "requires two Stalwart instances (./tests/stalwart2.sh) on :143/:144 and --ignored"]
fn a_duplicated_identity_syncs_both_copies_and_settles() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    let eml = root.join("msg.eml");
    let marker = marker();
    fs::create_dir_all(&state).unwrap();
    fs::write(&eml, message(&marker)).unwrap();

    // 1. One copy on A, two on B.
    append(&eml, A);
    append(&eml, B);
    append(&eml, B);
    assert_eq!(uids(B, &marker).len(), 2, "B holds the message twice");

    fs::write(&config, config_toml()).unwrap();
    neverest(&["init", "-a", ACCOUNT], &config, &state);

    // Both copies are items, so the one A lacks crosses like any other, and
    // nothing about the pair is worth a warning.
    let report = sync(&config, &state);
    assert!(
        report.get("ambiguous").is_none(),
        "nothing is ambiguous any more: {report}"
    );
    assert!(
        report.get("refused").is_none(),
        "an IMAP server holds no UID to refuse: {report}"
    );
    assert_eq!(
        uids(A, &marker).len(),
        2,
        "the second copy was appended to A: {report}"
    );

    // 2. Nothing changed, so nothing is reported. This is the run the freeze
    //    could never reach: a copy with no row was read as a body still to
    //    fetch, and named again on every run for ever.
    let report = sync(&config, &state);
    assert_eq!(
        report["item"][""].as_array().map_or(0, Vec::len),
        0,
        "a settled collection reports nothing: {report}"
    );

    // 3. One copy goes on B. It is an item of its own now, so its delete says
    //    something exact: that copy goes on A too, and the other stays.
    let mut expected = uids(B, &marker);
    expected.sort();
    expunge(B, &expected[0]);
    assert_eq!(uids(B, &marker).len(), 1, "B now holds the message once");
    let report = sync(&config, &state);
    assert_eq!(
        uids(A, &marker).len(),
        1,
        "exactly the deleted copy went: {report}"
    );

    // 4. B loses its checkpoint, so its next enumeration is full. A bound
    //    handle keeps the key it was given, so nothing is minted twice and
    //    nothing is re-appended.
    drop_checkpoint(&state, "right");
    let report = sync(&config, &state);
    assert_eq!(
        uids(A, &marker).len(),
        1,
        "the full enumeration re-appended nothing to A: {report}"
    );
    assert_eq!(uids(B, &marker).len(), 1, "nor to B: {report}");
}

fn config_toml() -> String {
    format!(
        "[accounts.{ACCOUNT}]\n\
         sources.left.imap.server = \"{A}\"\n\
         sources.left.imap.starttls = false\n\
         sources.left.imap.sasl.plain.username = \"test@pimalaya.org\"\n\
         sources.left.imap.sasl.plain.password.raw = \"P!malaya-test-2026\"\n\
         targets.right.imap.server = \"{B}\"\n\
         targets.right.imap.starttls = false\n\
         targets.right.imap.sasl.plain.username = \"test@pimalaya.org\"\n\
         targets.right.imap.sasl.plain.password.raw = \"P!malaya-test-2026\"\n",
    )
}

/// Empties one side's stored sync cursor, which the IMAP backend reads as an
/// absent checkpoint and answers with a full enumeration: the shape a
/// UIDVALIDITY bump, a server without QRESYNC or a reset leaves behind.
fn drop_checkpoint(state: &Path, source: &str) {
    let dir = state.join("neverest").join(ACCOUNT);
    let mut store = PimdirStore::open(&dir)
        .expect("open the account store")
        .for_account(ACCOUNT)
        .for_source(source);
    io_replica::client::ReplicaStorage::write(
        &mut store,
        vec![ReplicaWriteOp::SetCheckpoint {
            collection: ReplicaCollectionId("left/INBOX".into()),
            checkpoint: ReplicaCheckpoint(Vec::new()),
        }],
    )
    .expect("drop the checkpoint");
}

/// APPENDs `eml` to `url`'s mailbox.
fn append(eml: &Path, url: &str) {
    let output = Command::new("curl")
        .args(["-fsS", "-T"])
        .arg(eml)
        .args([url, "--user", CRED])
        .output()
        .expect("spawn curl append");
    assert!(
        output.status.success(),
        "curl APPEND to {url} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// The UIDs `url`'s mailbox holds for this run's message.
fn uids(url: &str, marker: &str) -> Vec<String> {
    let output = Command::new("curl")
        .args([
            "-fsS",
            "--url",
            url,
            "--user",
            CRED,
            "-X",
            &format!("UID SEARCH TEXT {marker}"),
        ])
        .output()
        .expect("spawn curl search");
    assert!(
        output.status.success(),
        "curl SEARCH on {url} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter(|token| token.parse::<u32>().is_ok())
        .map(str::to_string)
        .collect()
}

/// Flags `uid` deleted in `url`'s mailbox and expunges that copy alone
/// (UIDPLUS), so what earlier runs left there is untouched.
fn expunge(url: &str, uid: &str) {
    for request in [
        format!("UID STORE {uid} +Flags \\Deleted"),
        format!("UID EXPUNGE {uid}"),
    ] {
        let output = Command::new("curl")
            .args(["-fsS", "--url", url, "--user", CRED, "-X", &request])
            .output()
            .expect("spawn curl store/expunge");
        assert!(
            output.status.success(),
            "curl {request} on {url} failed: {}",
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

/// Syncs the account and returns the parsed `--json` report.
fn sync(config: &Path, state: &Path) -> serde_json::Value {
    let stdout = neverest(&["--json", "sync", "-a", ACCOUNT], config, state);
    serde_json::from_str(&stdout).expect("the report is JSON")
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
