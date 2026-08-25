//! End-to-end proof of the duplicate-link-id freeze, against TWO local
//! Stalwart IMAP servers (A :143, B :144) spawned by `tests/stalwart2.sh`.
//! Ignored by default.
//!
//! One collection may hold two messages with the same `Message-ID`. Before the
//! freeze that cost mail on a side the user never touched, in three steps, and
//! this test replays all three:
//!   1. Seed one copy on A and two on B, then sync. The identity is frozen:
//!      no hunk is derived for it, and the report warns naming both UIDs.
//!   2. Delete one copy on B and sync. A's copy survives, and no delete is
//!      pushed to it.
//!   3. Drop B's checkpoint (a UIDVALIDITY bump, a server without QRESYNC, a
//!      reset) and sync. The full enumeration must not re-append to A.
//!
//! Detection, the derive-nothing rules and the persistence are upstream
//! (io-replica and io-pimdir, same change id); what is proved here is that
//! they hold end to end through this crate, and that the user is told.
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
/// to report rather than a fault to blame.
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
fn a_duplicated_identity_is_frozen_reported_and_never_costs_the_other_side_its_copy() {
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

    // The identity is frozen: nothing is derived for it, and the report names
    // the collection and both UIDs on B.
    let report = sync(&config, &state);
    assert_eq!(
        report["item"]["patch"].as_array().map_or(0, Vec::len),
        0,
        "no hunk is derived for a frozen identity: {report}"
    );
    let mut expected = uids(B, &marker);
    expected.sort();
    assert_eq!(warned_handles(&report, "right"), expected, "{report}");

    // 2. One copy goes on B. The delete must not propagate: the copy that
    //    vanished says nothing about the one that did not, and acting on that
    //    reading is what used to remove the only copy on A.
    expunge(B, &expected[0]);
    assert_eq!(uids(B, &marker).len(), 1, "B now holds the message once");
    let report = sync(&config, &state);
    assert!(
        !mentions_delete(&report),
        "no delete is pushed on the word of a source that held the identity twice: {report}"
    );
    assert_eq!(uids(A, &marker).len(), 1, "A still holds its copy");

    // 3. B loses its checkpoint, so its next enumeration is full. A retained
    //    row must not be revived into an append to A.
    drop_checkpoint(&state, "right");
    let report = sync(&config, &state);
    assert_eq!(
        uids(A, &marker).len(),
        1,
        "the full enumeration re-appended nothing to A: {report}"
    );
}

/// The handles the report's warning for `side` names, sorted.
fn warned_handles(report: &serde_json::Value, side: &str) -> Vec<String> {
    let warning = report["ambiguous"]
        .as_array()
        .expect("the report carries the warnings")
        .iter()
        .find(|warning| warning["side"] == side)
        .unwrap_or_else(|| panic!("`{side}` is reported as ambiguous"));
    assert_eq!(warning["collection"], "INBOX");

    let mut handles: Vec<String> = warning["ids"]
        .as_array()
        .expect("the warning names every handle")
        .iter()
        .map(|id| id.as_str().unwrap().to_string())
        .collect();
    handles.sort();
    handles
}

fn config_toml() -> String {
    format!(
        "[accounts.{ACCOUNT}]\n\
         left.imap.server = \"{A}\"\n\
         left.imap.starttls = false\n\
         left.imap.sasl.plain.username = \"test@pimalaya.org\"\n\
         left.imap.sasl.plain.password.raw = \"P!malaya-test-2026\"\n\
         right.imap.server = \"{B}\"\n\
         right.imap.starttls = false\n\
         right.imap.sasl.plain.username = \"test@pimalaya.org\"\n\
         right.imap.sasl.plain.password.raw = \"P!malaya-test-2026\"\n",
    )
}

/// Whether the report carries any item hunk deleting a copy.
fn mentions_delete(report: &serde_json::Value) -> bool {
    report["item"]["patch"]
        .as_array()
        .is_some_and(|patch| patch.iter().any(|entry| entry["hunk"]["kind"] == "delete"))
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
            collection: ReplicaCollectionId("INBOX".into()),
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
            "curl `{request}` on {url} failed: {}",
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
