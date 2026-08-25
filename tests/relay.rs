//! End-to-end relay test against TWO local Stalwart IMAP servers (A :143, B
//! :144), spawned via `tests/stalwart2.sh`. Ignored by default.
//!
//! Proves the two-source pass-through: a message on server A is relayed to
//! server B by *streaming* it server-to-server, the pimdir store keeping only the
//! spine — no body blob at rest. Steps:
//!   1. Append a message to server A (via `curl` IMAP).
//!   2. Sync A ↔ B with `store.retention = "relay"`.
//!   3. Assert server B has the message (via `curl` IMAP SEARCH).
//!   4. Assert the relay account's store kept NO object blob (pure spine).
//!
//! Seeding/verifying uses `curl` rather than a local backend, since neverest no
//! longer has one (m2dir/maildir sides were dropped — it manages remotes only).
//!
//! Start the servers and run with:
//! ```sh
//! ./tests/stalwart2.sh
//! cargo test --test relay -- --ignored
//! ```

use std::{fs, path::Path, process::Command};

const A: &str = "imap://127.0.0.1:143/INBOX";
const B: &str = "imap://127.0.0.1:144/INBOX";
const CRED: &str = "test@pimalaya.org:P!malaya-test-2026";
/// A single-token marker so the IMAP `SEARCH TEXT` needs no quoting.
const MARKER: &str = "RELAYMARKER42";

fn message() -> Vec<u8> {
    format!(
        "Message-ID: <relay-1@pimalaya.org>\r\n\
         From: alice@pimalaya.org\r\n\
         To: bob@pimalaya.org\r\n\
         Subject: neverest relay roundtrip\r\n\
         Date: Tue, 28 Jul 2026 10:00:00 +0000\r\n\
         \r\n\
         {MARKER}\r\n",
    )
    .into_bytes()
}

#[test]
#[ignore = "requires two Stalwart instances (./tests/stalwart2.sh) on :143/:144 and --ignored"]
fn relay_streams_a_to_b_without_retaining() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    let eml = root.join("msg.eml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&eml, message()).unwrap();

    // 1. Seed server A with the message (IMAP APPEND via curl).
    let append = Command::new("curl")
        .args(["-fsS", "-T"])
        .arg(&eml)
        .args([A, "--user", CRED])
        .output()
        .expect("spawn curl append");
    assert!(
        append.status.success(),
        "curl APPEND to A failed: {}",
        String::from_utf8_lossy(&append.stderr),
    );

    // 2. Relay A ↔ B.
    let config_toml = format!(
        "[accounts.relay]\n\
         sources.left.imap.server = \"{A}\"\n\
         sources.left.imap.starttls = false\n\
         sources.left.imap.sasl.plain.username = \"test@pimalaya.org\"\n\
         sources.left.imap.sasl.plain.password.raw = \"P!malaya-test-2026\"\n\
         sources.right.imap.server = \"{B}\"\n\
         sources.right.imap.starttls = false\n\
         sources.right.imap.sasl.plain.username = \"test@pimalaya.org\"\n\
         sources.right.imap.sasl.plain.password.raw = \"P!malaya-test-2026\"\n\
         sources.left.imap.collection.namespace = \"mail\"\n\
         sources.right.imap.collection.namespace = \"mail\"\n",
    );
    fs::write(&config, config_toml).unwrap();
    neverest(&["init", "-a", "relay"], &config, &state);
    neverest(&["sync", "-a", "relay"], &config, &state);

    // 3. Server B now holds the message (IMAP SEARCH via curl).
    let search = Command::new("curl")
        .args([
            "-fsS",
            "--url",
            B,
            "--user",
            CRED,
            "-X",
            &format!("SEARCH TEXT {MARKER}"),
        ])
        .output()
        .expect("spawn curl search");
    let hits = String::from_utf8_lossy(&search.stdout);
    assert!(
        hits.split_whitespace().any(|t| t.parse::<u32>().is_ok()),
        "server B has the relayed message (SEARCH returned `{}`)",
        hits.trim(),
    );

    // 4. The relay account's store kept only the spine — no object blob at rest.
    let objects = state.join("neverest").join("relay").join("objects");
    let blobs = count_files(&objects);
    assert_eq!(
        blobs,
        0,
        "relay retained no body blob (pure pass-through), found {blobs} in {}",
        objects.display(),
    );
}

/// Counts regular files anywhere under `dir` (the sharded blob layout).
fn count_files(dir: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += count_files(&path);
            } else if path.is_file() {
                count += 1;
            }
        }
    }
    count
}

fn neverest(args: &[&str], config: &Path, state: &Path) {
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
}
