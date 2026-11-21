//! # Moves
//!
//! End-to-end move test against a local Stalwart IMAP server (A :143),
//! spawned via tests/stalwart2.sh. Ignored by default.
//!
//! A staged move must land once in its target: were the pushes of two
//! connections to overlap, the target would upload it while the source
//! relocated it, holding it twice (pimdir SYNC §5). The race does not lose
//! every time, so the message bounces between two mailboxes for many runs.
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
        let source = format!("moves/imap/{from}");
        let target = format!("moves/imap/{to}");

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

/// A staged move lands once whichever of its two collections a run syncs
/// first.
///
/// Each run names one alone (`--include-collection`, as a caller syncing what
/// a watcher reported): the source first, then the target first, twice each
/// way, the message bouncing between the two.
#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :143 and --ignored"]
fn a_move_is_delivered_once_by_runs_naming_one_collection() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let store = root.join("store");
    let config = root.join("config.toml");

    let id = std::process::id();
    let left = format!("MoveOneLeft{id}");
    let right = format!("MoveOneRight{id}");
    let marker = format!("MOVEONEMARKER{id}");
    imap(&format!("CREATE {left}"));
    imap(&format!("CREATE {right}"));

    let eml = root.join("msg.eml");
    fs::write(
        &eml,
        format!(
            "Message-ID: <move-one-{id}@pimalaya.org>\r\n\
             From: alice@pimalaya.org\r\n\
             To: bob@pimalaya.org\r\n\
             Subject: neverest move by halves\r\n\
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
            "[accounts.halves]\n\
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
    neverest(&["init", "-a", "halves"], &config);
    neverest(&["sync", "-a", "halves"], &config);

    let mut from_first = true;
    let (mut from, mut to) = (left.clone(), right.clone());
    for bounce in 1..=4 {
        let source = format!("halves/imap/{from}");
        let target = format!("halves/imap/{to}");

        let mut producer = PimdirProducer::open(&store, "moves-test").unwrap();
        let items = producer_items(&store, &source);
        assert_eq!(items.len(), 1, "bounce {bounce}: {source} holds it once");
        let action = PimdirAction::Move {
            seq: items[0],
            to: PimdirCollectionId(target.clone()),
        };
        producer.enqueue(&source, &action, None).unwrap();
        drop(producer);

        let (first, second) = if from_first {
            (&from, &to)
        } else {
            (&to, &from)
        };
        neverest(&["sync", "-a", "halves", "-m", first], &config);
        neverest(&["sync", "-a", "halves", "-m", second], &config);

        assert_eq!(
            server_count(&to, &marker),
            1,
            "bounce {bounce} ({first} first): {to} holds the message once on the server"
        );
        assert_eq!(
            server_count(&from, &marker),
            0,
            "bounce {bounce} ({first} first): {from} is empty"
        );
        assert_eq!(
            producer_items(&store, &target).len(),
            1,
            "bounce {bounce} ({first} first): {target} holds the message once in the store"
        );
        assert!(
            producer_items(&store, &source).is_empty(),
            "bounce {bounce} ({first} first): {source} holds nothing in the store"
        );

        (from, to) = (to, from);
        if bounce % 2 == 0 {
            from_first = !from_first;
        }
    }

    imap(&format!("DELETE {left}"));
    imap(&format!("DELETE {right}"));
}

/// A move into a mailbox holding another message stating the same
/// `Message-ID` keeps both bodies on the server.
///
/// The target's message is listed before the move, or lands after the last
/// run that listed the target: the push then finds it unknown.
#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :143 and --ignored"]
fn a_move_into_a_message_stating_its_id_keeps_both() {
    for listed in [true, false] {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path();
        let store = root.join("store");
        let config = root.join("config.toml");

        let id = format!("{}{}", std::process::id(), u8::from(listed));
        let left = format!("SameIdLeft{id}");
        let right = format!("SameIdRight{id}");
        let moved = format!("MOVEDMARKER{id}");
        let other = format!("OTHERMARKER{id}");
        imap(&format!("CREATE {left}"));
        imap(&format!("CREATE {right}"));
        let message_id = format!("same-{id}@pimalaya.org");
        append(root, &left, &message_id, &moved);

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

        if listed {
            append(root, &right, &message_id, &other);
            neverest(&["sync", "-a", "moves"], &config);
        }

        let source = format!("moves/imap/{left}");
        let target = format!("moves/imap/{right}");
        let mut producer = PimdirProducer::open(&store, "moves-test").unwrap();
        let items = producer_items(&store, &source);
        assert_eq!(items.len(), 1, "listed {listed}: {source} holds it");
        let action = PimdirAction::Move {
            seq: items[0],
            to: PimdirCollectionId(target.clone()),
        };
        producer.enqueue(&source, &action, None).unwrap();
        drop(producer);

        if !listed {
            append(root, &right, &message_id, &other);
        }
        let runs = [
            synced(&["sync", "-a", "moves", "-m", &left], &config),
            synced(&["sync", "-a", "moves"], &config),
            synced(&["sync", "-a", "moves"], &config),
        ];
        let runs = runs.join("\n=====\n");

        let kept = server_count(&left, &moved) + server_count(&right, &moved);
        assert_eq!(kept, 1, "listed {listed}: the moved body survives\n{runs}");
        assert_eq!(
            server_count(&right, &other),
            1,
            "listed {listed}: the other body survives\n{runs}"
        );
        assert_eq!(
            producer_items(&store, &target).len(),
            2,
            "listed {listed}: {target} holds both in the store\n{runs}"
        );
        for (link_id, stored, served) in bound_bodies(&store, &target, &right) {
            assert_eq!(
                stored, served,
                "listed {listed}: {link_id} holds the body of the message it is bound to\n{runs}"
            );
        }

        imap(&format!("DELETE {left}"));
        imap(&format!("DELETE {right}"));
    }
}

/// Appends a message stating `message_id`, its body `marker`, to `mailbox`.
fn append(root: &Path, mailbox: &str, message_id: &str, marker: &str) {
    let eml = root.join(format!("{marker}.eml"));
    fs::write(
        &eml,
        format!(
            "Message-ID: <{message_id}>\r\n\
             From: alice@pimalaya.org\r\n\
             To: bob@pimalaya.org\r\n\
             Subject: neverest same id\r\n\
             Date: Sat, 03 Oct 2026 10:00:00 +0000\r\n\
             \r\n\
             {marker}\r\n",
        ),
    )
    .unwrap();
    let appended = Command::new("curl")
        .args(["-fsS", "-T"])
        .arg(&eml)
        .arg(format!("{SERVER}/{mailbox}"))
        .args(["--user", CRED])
        .output()
        .expect("spawn curl append");
    assert!(appended.status.success(), "append to {mailbox} failed");
}

/// Runs neverest, answering its exit code and output whatever they are.
fn synced(args: &[&str], config: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(args)
        .output()
        .expect("spawn neverest");
    format!(
        "`neverest {}`: {}\n{}\n{}",
        args.join(" "),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

/// Each live item of `collection`, its link id, the marker its stored body
/// carries and the one the server's message of its handle in `mailbox` does.
fn bound_bodies(store: &Path, collection: &str, mailbox: &str) -> Vec<(String, String, String)> {
    let reader = io_pimdir::client::reader::PimdirReader::open(store).unwrap();
    let marker = |body: &str| {
        body.split_whitespace()
            .find(|word| word.contains("MARKER"))
            .unwrap_or_default()
            .to_string()
    };
    let mut bound = Vec::new();
    for item in reader.list_items(collection, None, 100).unwrap() {
        let mut stored = String::new();
        if let Some(hash) = &item.object {
            let mut blob = reader.blobs().reader(hash).unwrap().expect("a body");
            std::io::Read::read_to_string(&mut blob, &mut stored).unwrap();
        }
        let handles = reader
            .item_bindings(collection, &item.link_id.0)
            .unwrap()
            .into_values()
            .map(|binding| binding.handle.0)
            .collect::<Vec<_>>();
        assert_eq!(handles.len(), 1, "{}: {handles:?}", item.link_id.0);
        let output = Command::new("curl")
            .args(["-fsS", "--user", CRED])
            .arg(format!("{SERVER}/{mailbox};UID={}", handles[0]))
            .output()
            .expect("spawn curl fetch");
        assert!(output.status.success(), "fetch {mailbox} {}", handles[0]);
        let served = String::from_utf8_lossy(&output.stdout).into_owned();
        bound.push((item.link_id.0, marker(&stored), marker(&served)));
    }
    bound
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
