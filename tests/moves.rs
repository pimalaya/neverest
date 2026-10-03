//! End-to-end move test against a local Stalwart IMAP server (A :143), spawned
//! via `tests/stalwart2.sh`. Ignored by default.
//!
//! A move staged through the store must land once in its target. The run scans
//! its collections over several connections; were their pushes to overlap, the
//! target would upload the message while the source relocated it, and the
//! target would hold it twice (pimdir SYNC §5). The race does not lose every
//! time, so the message bounces between two mailboxes for several runs, each
//! checked on the server and in the store.
//!
//! ```sh
//! ./tests/stalwart2.sh
//! cargo test --test moves -- --ignored
//! ```

use std::{fs, path::Path, process::Command};

use io_pimdir::{
    client::producer::PimdirProducer, codec::PimdirAction, collection::PimdirCollectionId,
};

const SERVER: &str = "imap://127.0.0.1:143";
const CRED: &str = "test@pimalaya.org:P!malaya-test-2026";
const BOUNCES: usize = 8;

#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :143 and --ignored"]
fn a_move_is_delivered_once() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let store = root.join("store");
    let config = root.join("config.toml");

    let id = std::process::id();
    let left = format!("MoveLeft{id}");
    let right = format!("MoveRight{id}");
    let marker = format!("MOVEMARKER{id}");
    imap(&format!("CREATE {left}"));
    imap(&format!("CREATE {right}"));

    let eml = root.join("msg.eml");
    fs::write(
        &eml,
        format!(
            "Message-ID: <move-{id}@pimalaya.org>\r\n\
             From: alice@pimalaya.org\r\n\
             To: bob@pimalaya.org\r\n\
             Subject: neverest move once\r\n\
             Date: Sat, 03 Oct 2026 10:00:00 +0000\r\n\
             \r\n\
             {marker}\r\n",
        ),
    )
    .unwrap();
    let append = Command::new("curl")
        .args(["-fsS", "-T"])
        .arg(&eml)
        .arg(format!("{SERVER}/{left}"))
        .args(["--user", CRED])
        .output()
        .expect("spawn curl append");
    assert!(append.status.success(), "seed append failed");

    fs::write(
        &config,
        format!(
            "[accounts.moves]\n\
             store.root = \"{}\"\n\
             imap.server = \"{SERVER}\"\n\
             imap.starttls = false\n\
             imap.sasl.plain.username = \"test@pimalaya.org\"\n\
             imap.sasl.plain.password.raw = \"P!malaya-test-2026\"\n\
             imap.collection.filter.include = [\"{left}\", \"{right}\"]\n",
            store.display(),
        ),
    )
    .unwrap();
    neverest(&["init", "-a", "moves"], &config);
    neverest(&["sync", "-a", "moves"], &config);

    let (mut from, mut to) = (left.clone(), right.clone());
    for bounce in 1..=BOUNCES {
        let source = format!("imap/{from}");
        let target = format!("imap/{to}");

        let mut producer = PimdirProducer::open(&store, "moves-test").unwrap();
        let items = producer_items(&store, &source);
        assert_eq!(
            items.len(),
            1,
            "bounce {bounce}: {source} holds the message once"
        );
        let action = PimdirAction::Move {
            seq: items[0],
            to: PimdirCollectionId(target.clone()),
        };
        producer.enqueue(&source, &action, None).unwrap();
        drop(producer);

        neverest(&["sync", "-a", "moves"], &config);

        assert_eq!(
            server_count(&to, &marker),
            1,
            "bounce {bounce}: {to} holds the message once on the server"
        );
        assert_eq!(
            server_count(&from, &marker),
            0,
            "bounce {bounce}: {from} is empty"
        );
        assert_eq!(
            producer_items(&store, &target).len(),
            1,
            "bounce {bounce}: {target} holds the message once in the store"
        );

        (from, to) = (to, from);
    }

    imap(&format!("DELETE {left}"));
    imap(&format!("DELETE {right}"));
}

/// The live items of `collection`, by seq.
fn producer_items(store: &Path, collection: &str) -> Vec<i64> {
    let reader = io_pimdir::client::reader::PimdirReader::open(store).unwrap();
    reader
        .list_items(collection, None, 100)
        .unwrap()
        .into_iter()
        .map(|item| item.seq)
        .collect()
}

/// How many messages of `mailbox` carry `marker`, through IMAP `SEARCH`.
fn server_count(mailbox: &str, marker: &str) -> usize {
    let output = Command::new("curl")
        .args(["-fsS", "--user", CRED])
        .arg(format!("{SERVER}/{mailbox}"))
        .args(["-X", &format!("SEARCH TEXT {marker}")])
        .output()
        .expect("spawn curl search");
    assert!(output.status.success(), "search {mailbox} failed");
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter(|word| word.parse::<u32>().is_ok())
        .count()
}

/// Runs one IMAP command on the server, failing the test if it fails.
fn imap(command: &str) {
    let output = Command::new("curl")
        .args(["-fsS", "--user", CRED, SERVER, "-X", command])
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
