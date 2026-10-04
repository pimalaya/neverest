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
