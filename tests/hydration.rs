//! End-to-end proof that a one-source sync keeps each body as it arrives,
//! against a local Stalwart IMAP server (A :143), spawned via
//! `tests/stalwart.sh`. Ignored by default.
//!
//! A local sync used to raise every body to `Full` only once the whole account
//! had downloaded, so no mail was readable before then and a run stopped
//! halfway kept nothing. Steps:
//!   1. Seed three folders with messages large enough for the download to
//!      take a moment over one connection.
//!   2. Sync over one connection, each folder being one batch, and kill the
//!      run halfway through the second batch. The first batch is readable
//!      already, though the account has not finished downloading.
//!   3. Sync again. The run fetches exactly the bodies the first one did not
//!      keep, and every message of every folder ends up with its body.
//!
//! Start the server and run with:
//! ```sh
//! ./tests/stalwart.sh
//! cargo test --test hydration -- --ignored
//! ```

use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use io_pimdir::client::reader::PimdirReader;

const SERVER: &str = "imap://127.0.0.1:143";
const CRED: &str = "test@pimalaya.org:P!malaya-test-2026";

/// Messages per folder.
const MESSAGES: usize = 30;

/// Body size of each message, in bytes.
const BODY: usize = 1024 * 1024;

#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart.sh) on :143 and --ignored"]
fn a_body_is_kept_as_it_arrives() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let store = root.join("store");
    let config = root.join("config.toml");

    let id = std::process::id();
    let folders: Vec<String> = (1..=3).map(|n| format!("Hydrate{id}x{n}")).collect();
    let line = "x".repeat(76);
    let body: String = (0..BODY / 78).map(|_| format!("{line}\r\n")).collect();
    let eml = root.join("msg.eml");
    for folder in &folders {
        imap(&format!("CREATE {folder}"));
        for n in 0..MESSAGES {
            fs::write(
                &eml,
                format!(
                    "Message-ID: <hydrate-{id}-{folder}-{n}@pimalaya.org>\r\n\
                     From: alice@pimalaya.org\r\n\
                     To: bob@pimalaya.org\r\n\
                     Subject: neverest hydration {n}\r\n\
                     Date: Sun, 04 Oct 2026 10:00:00 +0000\r\n\
                     \r\n\
                     {body}",
                ),
            )
            .unwrap();
            let append = Command::new("curl")
                .args(["-fsS", "-T"])
                .arg(&eml)
                .arg(format!("{SERVER}/{folder}"))
                .args(["--user", CRED])
                .output()
                .expect("spawn curl append");
            assert!(append.status.success(), "seed append failed");
        }
    }
    let total = folders.len() * MESSAGES;

    let include: Vec<String> = folders.iter().map(|f| format!("\"{f}\"")).collect();
    fs::write(
        &config,
        format!(
            "[accounts.hydrate]\n\
             store.root = \"{}\"\n\
             imap.server = \"{SERVER}\"\n\
             imap.starttls = false\n\
             imap.sasl.plain.username = \"test@pimalaya.org\"\n\
             imap.sasl.plain.password.raw = \"P!malaya-test-2026\"\n\
             imap.collection.filter.include = [{}]\n",
            store.display(),
            include.join(", "),
        ),
    )
    .unwrap();
    neverest(&["init", "-a", "hydrate"], &config);

    let mut first = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(["sync", "-a", "hydrate", "-j", "1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn neverest");
    let deadline = Instant::now() + Duration::from_secs(120);
    while downloaded(&store) < MESSAGES + MESSAGES / 2 {
        assert!(
            Instant::now() < deadline,
            "the download did not reach the second batch"
        );
        assert!(
            first.try_wait().unwrap().is_none(),
            "the first run ended before the second batch was half downloaded"
        );
        thread::sleep(Duration::from_millis(2));
    }
    first.kill().unwrap();
    first.wait().unwrap();

    let kept = bodies(&store, &folders);
    assert!(
        kept >= MESSAGES,
        "the first batch is readable before the account has downloaded ({kept} kept)"
    );
    assert!(
        kept < total,
        "the run was stopped before the end ({kept}/{total})"
    );

    let report = neverest(&["--json", "sync", "-a", "hydrate"], &config);
    let report: serde_json::Value = serde_json::from_str(&report).expect("a JSON report");
    let fetched = report["item"]["patch"]
        .as_array()
        .expect("item patch")
        .iter()
        .filter(|entry| entry["hunk"]["kind"] == "fetch")
        .count();
    assert_eq!(
        fetched,
        total - kept,
        "the second run fetches only what the first did not keep"
    );
    assert_eq!(bodies(&store, &folders), total, "every body is stored");

    for folder in &folders {
        imap(&format!("DELETE {folder}"));
    }
}

/// Under `--download-order newest`, recent mail downloads first even when an
/// older folder holds the larger bodies, which the default order fetches
/// first: over one connection, the first batch applied is the recent one.
#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart.sh) on :143 and --ignored"]
fn recent_mail_downloads_first_on_request() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let store = root.join("store");
    let config = root.join("config.toml");

    let id = std::process::id();
    let old = format!("Newest{id}Old");
    let recent = format!("Newest{id}Recent");
    let line = "x".repeat(76);
    let large: String = (0..BODY / 78).map(|_| format!("{line}\r\n")).collect();
    let eml = root.join("msg.eml");
    for (folder, date, body) in [
        (&old, "Wed, 01 May 2019 10:00:00 +0000", large.as_str()),
        (&recent, "Sun, 04 Oct 2026 10:00:00 +0000", "hello\r\n"),
    ] {
        imap(&format!("CREATE {folder}"));
        for n in 0..MESSAGES {
            fs::write(
                &eml,
                format!(
                    "Message-ID: <newest-{id}-{folder}-{n}@pimalaya.org>\r\n\
                     From: alice@pimalaya.org\r\n\
                     To: bob@pimalaya.org\r\n\
                     Subject: neverest order {n}\r\n\
                     Date: {date}\r\n\
                     \r\n\
                     {body}",
                ),
            )
            .unwrap();
            let append = Command::new("curl")
                .args(["-fsS", "-T"])
                .arg(&eml)
                .arg(format!("{SERVER}/{folder}"))
                .args(["--user", CRED])
                .output()
                .expect("spawn curl append");
            assert!(append.status.success(), "seed append failed");
        }
    }

    fs::write(
        &config,
        format!(
            "[accounts.newest]\n\
             store.root = \"{}\"\n\
             imap.server = \"{SERVER}\"\n\
             imap.starttls = false\n\
             imap.sasl.plain.username = \"test@pimalaya.org\"\n\
             imap.sasl.plain.password.raw = \"P!malaya-test-2026\"\n\
             imap.collection.filter.include = [\"{old}\", \"{recent}\"]\n",
            store.display(),
        ),
    )
    .unwrap();
    neverest(&["init", "-a", "newest"], &config);

    let mut run = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args([
            "sync",
            "-a",
            "newest",
            "-j",
            "1",
            "--download-order",
            "newest",
            // NOTE: the order alone, across the account: priming would take
            // each folder's newest first, the old folder's among them.
            "--prime",
            "0",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn neverest");
    let folders = [old.clone(), recent.clone()];
    let deadline = Instant::now() + Duration::from_secs(120);
    while bodies(&store, &folders) < MESSAGES {
        assert!(Instant::now() < deadline, "no batch was applied in time");
        assert!(
            run.try_wait().unwrap().is_none(),
            "the run ended before it could be stopped"
        );
        thread::sleep(Duration::from_millis(2));
    }
    run.kill().unwrap();
    run.wait().unwrap();

    assert_eq!(
        bodies(&store, std::slice::from_ref(&recent)),
        MESSAGES,
        "the recent folder's batch came first"
    );
    assert!(
        bodies(&store, std::slice::from_ref(&old)) < MESSAGES,
        "the old, larger folder came after"
    );

    neverest(
        &["sync", "-a", "newest", "--download-order", "newest"],
        &config,
    );
    assert_eq!(
        bodies(&store, &folders),
        2 * MESSAGES,
        "every body is stored"
    );

    for folder in &folders {
        imap(&format!("DELETE {folder}"));
    }
}

/// Messages in each folder of the priming tests: over two IMAP pages.
const LARGE: usize = 1_100;

/// How many of each folder's newest bodies the priming tests ask for.
const PRIME: usize = 50;

/// Seeds `folders` with [`LARGE`] small messages each, dated a minute
/// apart in the order they are appended, so the newest has the highest UID.
fn seed_large(folders: &[String], tag: &str) {
    for folder in folders {
        imap(&format!("CREATE {folder}"));
        let messages = (0..LARGE).map(|n| {
            let minute = n % 60;
            let hour = (n / 60) % 24;
            let day = 1 + n / (60 * 24);
            format!(
                "Message-ID: <{tag}-{folder}-{n:04}@pimalaya.org>\r\n\
                 From: alice@pimalaya.org\r\n\
                 To: bob@pimalaya.org\r\n\
                 Subject: neverest prime {n}\r\n\
                 Date: {day:02} Oct 2026 {hour:02}:{minute:02}:00 +0000\r\n\
                 \r\n\
                 hello {n}\r\n"
            )
        });
        append_all(folder, messages);
    }
}

/// Appends `messages` to `folder` over one IMAP session, curl taking one
/// session a message.
fn append_all(folder: &str, messages: impl Iterator<Item = String>) {
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpStream,
    };

    let stream = TcpStream::connect("127.0.0.1:143").expect("connect IMAP");
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;
    let mut line = String::new();
    let mut expect = |reader: &mut BufReader<TcpStream>, start: &str| loop {
        line.clear();
        reader.read_line(&mut line).expect("read IMAP");
        assert!(!line.is_empty(), "IMAP closed the connection");
        if line.split_whitespace().next() == Some(start) {
            assert!(
                start == "*" || line.starts_with(&format!("{start} OK")),
                "IMAP refused: {line}"
            );
            return;
        }
    };
    expect(&mut reader, "*");
    let (user, pass) = CRED.split_once(':').unwrap();
    write!(writer, "a LOGIN \"{user}\" \"{pass}\"\r\n").unwrap();
    expect(&mut reader, "a");
    // NOTE: a hundred messages a command (MULTIAPPEND, RFC 3502), in
    // non-synchronizing literals (RFC 7888): one command appends its
    // messages in order, so the UIDs follow the dates, where pipelined
    // commands may be served out of order.
    let messages: Vec<String> = messages.collect();
    for (chunk, messages) in messages.chunks(100).enumerate() {
        write!(writer, "b{chunk} APPEND {folder}").unwrap();
        for message in messages {
            write!(writer, " {{{}+}}\r\n{message}", message.len()).unwrap();
        }
        write!(writer, "\r\n").unwrap();
        expect(&mut reader, &format!("b{chunk}"));
    }
    write!(writer, "z LOGOUT\r\n").unwrap();
}

/// The live items of `folder`, by link id.
fn items(store: &Path, folder: &str) -> Vec<io_pimdir::client::reader::PimdirItem> {
    let Ok(reader) = PimdirReader::open(store) else {
        return Vec::new();
    };
    reader
        .list_items(format!("imap/{folder}"), None, 10_000)
        .unwrap_or_default()
}

/// Whether message `n` of `folder` is held with its body.
fn has_body(items: &[io_pimdir::client::reader::PimdirItem], folder: &str, n: usize) -> bool {
    let suffix = format!("-{folder}-{n:04}@pimalaya.org");
    items
        .iter()
        .any(|item| item.link_id.0.contains(&suffix) && item.object.is_some())
}

fn prime_config(root: &Path, account: &str, folders: &[String]) -> std::path::PathBuf {
    let config = root.join(format!("{account}.toml"));
    let include: Vec<String> = folders.iter().map(|f| format!("\"{f}\"")).collect();
    fs::write(
        &config,
        format!(
            "[accounts.{account}]\n\
             store.root = \"{}\"\n\
             imap.server = \"{SERVER}\"\n\
             imap.starttls = false\n\
             imap.sasl.plain.username = \"test@pimalaya.org\"\n\
             imap.sasl.plain.password.raw = \"P!malaya-test-2026\"\n\
             imap.collection.filter.include = [{}]\n",
            root.join(account).display(),
            include.join(", "),
        ),
    )
    .unwrap();
    neverest(&["init", "-a", account], &config);
    config
}

/// Priming lists every folder's first page before any second page, each
/// followed by its newest bodies, over one connection; a run killed then
/// leaves every folder readable from its newest mail, and the next run
/// closes every round, each message held once with its body.
#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart.sh) on :143 and --ignored"]
fn every_folder_is_readable_from_its_newest_mail_early() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let id = std::process::id();
    let folders: Vec<String> = ["a", "b", "c"]
        .iter()
        .map(|n| format!("Prime{id}{n}"))
        .collect();
    seed_large(&folders, "prime");
    let config = prime_config(root, "prime", &folders);
    let store = root.join("prime");

    let mut run = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args([
            "sync",
            "-a",
            "prime",
            "-j",
            "1",
            "--download-order",
            "newest",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn neverest");
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut first_pages = None;
    loop {
        assert!(
            Instant::now() < deadline,
            "the first pass did not end in time"
        );
        assert!(
            run.try_wait().unwrap().is_none(),
            "the run ended before it could be stopped"
        );
        let held: Vec<_> = folders.iter().map(|f| items(&store, f)).collect();
        if first_pages.is_none() && !held[2].is_empty() {
            first_pages = Some([held[0].len(), held[1].len()]);
        }
        let primed = held
            .iter()
            .zip(&folders)
            .all(|(items, folder)| (LARGE - PRIME..LARGE).all(|n| has_body(items, folder, n)));
        if primed {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    run.kill().unwrap();
    run.wait().unwrap();

    assert_eq!(
        first_pages,
        Some([500, 500]),
        "the last folder's first page landed before any second page"
    );

    let report = neverest(&["--json", "sync", "-a", "prime"], &config);
    let report: serde_json::Value = serde_json::from_str(&report).expect("a JSON report");
    for coverage in report["coverage"].as_array().expect("coverage") {
        assert!(
            coverage["round"].is_null(),
            "every round closed: {coverage}"
        );
    }
    for folder in &folders {
        let held = items(&store, folder);
        assert_eq!(held.len(), LARGE, "{folder} holds each message once");
        assert!(
            held.iter().all(|item| item.object.is_some()),
            "{folder} holds every body"
        );
    }

    for folder in &folders {
        imap(&format!("DELETE {folder}"));
    }
}

/// A primed run that is not interrupted reports each body it fetched once,
/// the fetches of a paused folder carried to the spine that fills it, over
/// several connections as over one; and how soon the first body and every
/// first page are readable, primed or not (`--nocapture` prints it).
#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart.sh) on :143 and --ignored"]
fn a_primed_run_reports_each_fetch_once() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let id = std::process::id();
    let folders: Vec<String> = ["a", "b", "c"]
        .iter()
        .map(|n| format!("Report{id}{n}"))
        .collect();
    seed_large(&folders, "report");

    for (connections, prime) in [("1", "0"), ("1", "50"), ("4", "0"), ("4", "50")] {
        let account = format!("report{connections}x{prime}");
        let config = prime_config(root, &account, &folders);
        let store = root.join(&account);

        let start = Instant::now();
        let run = Command::new(env!("CARGO_BIN_EXE_neverest"))
            .args(["-c", &config.to_string_lossy()])
            .args(["--json", "sync", "-a", &account, "-j", connections])
            .args(["--download-order", "newest", "--prime", prime])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn neverest");
        let (mut first_body, mut first_pages) = (None, None);
        while first_body.is_none() || first_pages.is_none() {
            let held: Vec<_> = folders.iter().map(|f| items(&store, f)).collect();
            if first_body.is_none() && held.iter().flatten().any(|item| item.object.is_some()) {
                first_body = Some(start.elapsed());
            }
            if first_pages.is_none() && held.iter().all(|items| !items.is_empty()) {
                first_pages = Some(start.elapsed());
            }
            if start.elapsed() > Duration::from_secs(120) {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        let output = run.wait_with_output().expect("wait neverest");
        let whole = start.elapsed();
        assert!(output.status.success(), "{account} sync failed");
        eprintln!(
            "-j {connections} --prime {prime}: first body {first_body:?}, every first page {first_pages:?}, whole run {whole:?}"
        );

        let report: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("a JSON report");
        let fetched: Vec<&str> = report["item"]["patch"]
            .as_array()
            .expect("item patch")
            .iter()
            .filter(|entry| entry["hunk"]["kind"] == "fetch")
            .map(|entry| entry["hunk"]["id"].as_str().expect("an id"))
            .collect();
        let distinct: std::collections::BTreeSet<&str> = fetched.iter().copied().collect();
        assert_eq!(
            fetched.len(),
            folders.len() * LARGE,
            "{account}: one fetch a message"
        );
        assert_eq!(distinct.len(), fetched.len(), "{account}: none twice");
        for folder in &folders {
            let held = items(&store, folder);
            assert_eq!(held.len(), LARGE, "{account}: {folder} once");
            assert!(held.iter().all(|item| item.object.is_some()));
        }
    }

    for folder in &folders {
        imap(&format!("DELETE {folder}"));
    }
}

/// A run under `--bodies listed` downloads the body of a new mail and
/// leaves the bodies an interrupted run left owed for a later run.
#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart.sh) on :143 and --ignored"]
fn a_listed_run_leaves_the_backlog_owed() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let id = std::process::id();
    let folder = format!("Listed{id}");
    let line = "x".repeat(76);
    let large: String = (0..BODY / 78).map(|_| format!("{line}\r\n")).collect();
    imap(&format!("CREATE {folder}"));
    let message = |n: usize, body: &str| {
        format!(
            "Message-ID: <listed-{id}-{n:04}@pimalaya.org>\r\n\
             From: alice@pimalaya.org\r\n\
             To: bob@pimalaya.org\r\n\
             Subject: neverest listed {n}\r\n\
             Date: Sun, 04 Oct 2026 10:{n:02}:00 +0000\r\n\
             \r\n\
             {body}"
        )
    };
    append_all(&folder, (0..MESSAGES).map(|n| message(n, &large)));
    let folders = [folder.clone()];
    let config = prime_config(root, "listed", &folders);
    let store = root.join("listed");

    let mut first = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(["sync", "-a", "listed", "-j", "1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn neverest");
    let deadline = Instant::now() + Duration::from_secs(120);
    while items(&store, &folder).len() < MESSAGES {
        assert!(Instant::now() < deadline, "the page did not land in time");
        thread::sleep(Duration::from_millis(1));
    }
    first.kill().unwrap();
    first.wait().unwrap();
    let kept = bodies(&store, &folders);
    assert!(kept < MESSAGES, "bodies were left owed ({kept})");

    append_all(&folder, std::iter::once(message(MESSAGES, "hello\r\n")));
    neverest(
        &["sync", "-a", "listed", "-m", &folder, "--bodies", "listed"],
        &config,
    );

    let held = items(&store, &folder);
    assert_eq!(held.len(), MESSAGES + 1);
    assert!(has_body_of(&held, id, MESSAGES), "the new mail is readable");
    assert_eq!(bodies(&store, &folders), kept + 1, "the backlog stays owed");

    neverest(&["sync", "-a", "listed"], &config);
    assert_eq!(
        bodies(&store, &folders),
        MESSAGES + 1,
        "a full run takes it"
    );

    imap(&format!("DELETE {folder}"));
}

/// Messages expunged between the listing and the download of their
/// bodies leave the run complete, their members kept as listed; the next
/// run's listing takes them away and asks for nothing.
///
/// Over one connection, phase 2 downloads the largest bodies first, a
/// batch of 64 at a time: the tail of small messages sits in the second
/// batch, expunged while the first downloads.
#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart.sh) on :143 and --ignored"]
fn a_body_gone_mid_run_is_owed_nothing() {
    const HEADS: usize = 70;
    const TAILS: usize = 3;

    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let id = std::process::id();
    let folder = format!("Gone{id}");
    let line = "x".repeat(76);
    let large: String = (0..BODY / 78).map(|_| format!("{line}\r\n")).collect();
    imap(&format!("CREATE {folder}"));
    let message = |n: usize, body: &str| {
        format!(
            "Message-ID: <gone-{id}-{n:04}@pimalaya.org>\r\n\
             From: alice@pimalaya.org\r\n\
             To: bob@pimalaya.org\r\n\
             Subject: neverest gone {n}\r\n\
             Date: Sun, 04 Oct 2026 10:{:02}:00 +0000\r\n\
             \r\n\
             {body}",
            n % 60,
        )
    };
    // NOTE: twenty a command, under the server's request size.
    let messages: Vec<String> = (0..HEADS)
        .map(|n| message(n, &large))
        .chain((HEADS..HEADS + TAILS).map(|n| message(n, "small\r\n")))
        .collect();
    for chunk in messages.chunks(20) {
        append_all(&folder, chunk.iter().cloned());
    }
    let folders = [folder.clone()];
    let config = prime_config(root, "gone", &folders);
    let store = root.join("gone");

    let mut run = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(["sync", "-a", "gone", "-j", "1"])
        .stdout(Stdio::null())
        .stderr(fs::File::create(root.join("stderr")).unwrap())
        .spawn()
        .expect("spawn neverest");
    let deadline = Instant::now() + Duration::from_secs(120);
    while items(&store, &folder).len() < HEADS + TAILS {
        assert!(
            Instant::now() < deadline,
            "the listing did not land in time"
        );
        assert!(run.try_wait().unwrap().is_none(), "the run ended early");
        thread::sleep(Duration::from_millis(1));
    }
    // NOTE: the UIDs follow the appends, the tail last.
    let tail = format!("{}:{}", HEADS + 1, HEADS + TAILS);
    imap_in(&folder, &format!("UID STORE {tail} +FLAGS (\\Deleted)"));
    imap_in(&folder, "EXPUNGE");
    let status = run.wait().unwrap();
    assert!(
        status.success(),
        "a body gone fails no run:\n{}",
        fs::read_to_string(root.join("stderr")).unwrap_or_default()
    );

    let held = items(&store, &folder);
    assert_eq!(held.len(), HEADS + TAILS, "the gone members stay as listed");
    assert!(
        (0..HEADS).all(|n| has_body_of_tag(&held, "gone", id, n)),
        "every other body is stored"
    );
    assert!(
        (HEADS..HEADS + TAILS).all(|n| !has_body_of_tag(&held, "gone", id, n)),
        "the tail was expunged before its batch (raise HEADS if the run outran the expunge)"
    );

    let report = neverest(&["--json", "sync", "-a", "gone"], &config);
    let report: serde_json::Value = serde_json::from_str(&report).expect("a JSON report");
    let fetched = report["item"]["patch"]
        .as_array()
        .expect("item patch")
        .iter()
        .filter(|entry| entry["hunk"]["kind"] == "fetch")
        .count();
    assert_eq!(fetched, 0, "nothing asked again: {report}");
    let held = items(&store, &folder);
    assert_eq!(held.len(), HEADS, "the listing took the gone members away");
    assert!(held.iter().all(|item| item.object.is_some()));

    imap(&format!("DELETE {folder}"));
}

/// Runs one IMAP command in `folder`, failing the test if it fails.
fn imap_in(folder: &str, command: &str) {
    let output = Command::new("curl")
        .args(["-fsS", "--user", CRED])
        .arg(format!("{SERVER}/{folder}"))
        .args(["-X", command])
        .output()
        .expect("spawn curl");
    assert!(output.status.success(), "IMAP `{command}` failed");
}

/// Whether message `n` of a test tagged `tag` is held with its body.
fn has_body_of_tag(
    items: &[io_pimdir::client::reader::PimdirItem],
    tag: &str,
    id: u32,
    n: usize,
) -> bool {
    let suffix = format!("{tag}-{id}-{n:04}@pimalaya.org");
    items
        .iter()
        .any(|item| item.link_id.0.contains(&suffix) && item.object.is_some())
}

/// Whether message `n` of the listed test is held with its body.
fn has_body_of(items: &[io_pimdir::client::reader::PimdirItem], id: u32, n: usize) -> bool {
    let suffix = format!("listed-{id}-{n:04}@pimalaya.org");
    items
        .iter()
        .any(|item| item.link_id.0.contains(&suffix) && item.object.is_some())
}

/// How many messages of `folders` the store holds a body for.
fn bodies(store: &Path, folders: &[String]) -> usize {
    let Ok(reader) = PimdirReader::open(store) else {
        return 0;
    };
    folders
        .iter()
        .map(|folder| {
            reader
                .list_items(format!("imap/{folder}"), None, 10_000)
                .map(|items| items.iter().filter(|item| item.object.is_some()).count())
                .unwrap_or(0)
        })
        .sum()
}

/// How many bodies the store's blob tree holds, applied or not.
fn downloaded(store: &Path) -> usize {
    fn count(dir: &Path) -> usize {
        let Ok(entries) = fs::read_dir(dir) else {
            return 0;
        };
        entries
            .flatten()
            .map(|entry| match entry.file_type() {
                Ok(kind) if kind.is_dir() => count(&entry.path()),
                Ok(_) => 1,
                Err(_) => 0,
            })
            .sum()
    }
    count(&store.join("objects"))
}

/// Runs one IMAP command on the server, failing the test if it fails.
fn imap(command: &str) {
    let output = Command::new("curl")
        .args(["-fsS", "--user", CRED, SERVER, "-X", command])
        .output()
        .expect("spawn curl");
    assert!(output.status.success(), "IMAP `{command}` failed");
}

/// Runs neverest to completion, returning its stdout.
fn neverest(args: &[&str], config: &Path) -> String {
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
    String::from_utf8_lossy(&output.stdout).into_owned()
}
