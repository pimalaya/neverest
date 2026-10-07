//! End-to-end delete test against a local Stalwart IMAP server (:143), spawned
//! via `tests/stalwart.sh`. Ignored by default.
//!
//! A delete staged through the store removes that message alone. A plain
//! `EXPUNGE` would also remove every message another client marked `\Deleted`
//! in the mailbox and has not expunged yet, so neverest expunges by UID
//! (RFC 4315).
//!
//! ```sh
//! ./tests/stalwart.sh
//! cargo test --test deletes -- --ignored
//! ```

use std::{fs, path::Path, process::Command};

use io_pimdir::{client::producer::PimdirProducer, codec::PimdirAction};

const SERVER: &str = "imap://127.0.0.1:143";
const CRED: &str = "test@pimalaya.org:P!malaya-test-2026";

#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart.sh) on :143 and --ignored"]
fn a_delete_expunges_its_message_alone() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let store = root.join("store");
    let config = root.join("config.toml");

    let id = std::process::id();
    let mailbox = format!("Deletes{id}");
    let doomed = format!("DOOMED{id}");
    let kept = format!("KEPT{id}");
    imap(&format!("CREATE {mailbox}"));
    append(root, &mailbox, &doomed);
    append(root, &mailbox, &kept);

    fs::write(
        &config,
        format!(
            "[accounts.deletes]\n\
             store.root = \"{}\"\n\
             imap.server = \"{SERVER}\"\n\
             imap.starttls = false\n\
             imap.sasl.plain.username = \"test@pimalaya.org\"\n\
             imap.sasl.plain.password.raw = \"P!malaya-test-2026\"\n\
             imap.collection.filter.include = [\"{mailbox}\"]\n",
            store.display(),
        ),
    )
    .unwrap();
    neverest(&["init", "-a", "deletes"], &config);
    neverest(&["sync", "-a", "deletes"], &config);

    // Another client marks the kept message `\Deleted` and leaves it there.
    let kept_seq = server_seqs(&mailbox, &kept);
    assert_eq!(kept_seq.len(), 1, "the kept message is on the server");
    imap_in(
        &mailbox,
        &format!("STORE {} +FLAGS (\\Deleted)", kept_seq[0]),
    );

    let collection = format!("imap/{mailbox}");
    let reader = io_pimdir::client::reader::PimdirReader::open(&store).unwrap();
    let items = reader.list_summaries(&collection, None, 100).unwrap();
    drop(reader);
    assert_eq!(items.len(), 2, "both messages reached the store");
    let doomed_seq = items
        .iter()
        .find(|item| {
            item.summary
                .as_ref()
                .and_then(|summary| summary.hint())
                .is_some_and(|hint| hint.contains(&doomed))
        })
        .expect("the doomed message is in the store")
        .seq;

    let mut producer = PimdirProducer::open(&store, "deletes-test").unwrap();
    producer
        .enqueue(&collection, &PimdirAction::Remove { seq: doomed_seq }, None)
        .unwrap();
    drop(producer);
    neverest(&["sync", "-a", "deletes"], &config);

    assert!(
        server_seqs(&mailbox, &doomed).is_empty(),
        "the deleted message is gone from the server"
    );
    assert_eq!(
        server_seqs(&mailbox, &kept).len(),
        1,
        "the message another client marked \\Deleted is not expunged"
    );

    imap(&format!("DELETE {mailbox}"));
}

/// Appends a message whose subject and body carry `marker`.
fn append(root: &Path, mailbox: &str, marker: &str) {
    let eml = root.join(format!("{marker}.eml"));
    fs::write(
        &eml,
        format!(
            "Message-ID: <{marker}@pimalaya.org>\r\n\
             From: alice@pimalaya.org\r\n\
             To: bob@pimalaya.org\r\n\
             Subject: neverest delete {marker}\r\n\
             Date: Wed, 07 Oct 2026 10:00:00 +0000\r\n\
             \r\n\
             {marker}\r\n",
        ),
    )
    .unwrap();
    let output = Command::new("curl")
        .args(["-fsS", "-T"])
        .arg(&eml)
        .arg(format!("{SERVER}/{mailbox}"))
        .args(["--user", CRED])
        .output()
        .expect("spawn curl append");
    assert!(output.status.success(), "seed append failed");
}

/// The sequence numbers of the messages of `mailbox` carrying `marker`,
/// `\Deleted` ones included, through IMAP `SEARCH`.
fn server_seqs(mailbox: &str, marker: &str) -> Vec<u32> {
    let output = Command::new("curl")
        .args(["-fsS", "--user", CRED])
        .arg(format!("{SERVER}/{mailbox}"))
        .args(["-X", &format!("SEARCH TEXT {marker}")])
        .output()
        .expect("spawn curl search");
    assert!(output.status.success(), "search {mailbox} failed");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter_map(|word| word.parse::<u32>().ok())
        .collect()
}

/// Runs one IMAP command on the server, failing the test if it fails.
fn imap(command: &str) {
    let output = Command::new("curl")
        .args(["-fsS", "--user", CRED, SERVER, "-X", command])
        .output()
        .expect("spawn curl");
    assert!(output.status.success(), "IMAP `{command}` failed");
}

/// Runs one IMAP command with `mailbox` selected.
fn imap_in(mailbox: &str, command: &str) {
    let output = Command::new("curl")
        .args(["-fsS", "--user", CRED])
        .arg(format!("{SERVER}/{mailbox}"))
        .args(["-X", command])
        .output()
        .expect("spawn curl");
    assert!(output.status.success(), "IMAP `{command}` failed");
}

fn neverest(args: &[&str], config: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(args)
        .output()
        .expect("spawn neverest");
    assert!(
        output.status.success(),
        "`neverest {}` failed:\n--- stdout ---\n{}\n--- stderr ---\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
