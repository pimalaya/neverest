//! End-to-end scoped mail sync against a local Stalwart IMAP server (:143),
//! spawned via `tests/stalwart.sh`. Ignored by default.
//!
//! A scope bounds a mail collection by the `Date` header (pimdir SYNC §5):
//! the server narrows the listing with `SENTSINCE`, a day of margin below
//! the floor, and the `Date` decides. Every message here is appended today,
//! so its arrival says nothing: one dated 2020, one dated in the future, one
//! with no `Date` at all. Steps:
//!   1. Sync under `item.filter.since = "30d"`: the recent, the future and
//!      the undated messages are stored, the 2020 one is not.
//!   2. Widen with `--since 2019-01-01`: the 2020 message comes, the band
//!      below the coverage alone being listed.
//!   3. Narrow back: nothing is deleted, neither in the store nor on the
//!      server, absence meaning deleted only in scope.
//!   4. Delete the 2020 message on the server: its removal applies, out of
//!      scope as it is, and nothing else moves.
//!   5. A round of three pages killed after its first page resumes and
//!      lands every message once.
//!
//! ```sh
//! ./tests/stalwart.sh
//! cargo test --test scope -- --ignored
//! ```

use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use chrono::{Duration as Days, Utc};
use io_pimdir::client::reader::PimdirReader;

const SERVER: &str = "imap://127.0.0.1:143";
const CRED: &str = "test@pimalaya.org:P!malaya-test-2026";

#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart.sh) on :143 and --ignored"]
fn a_scope_lists_by_date_widens_and_never_deletes_what_it_leaves() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let store = root.join("store");
    let config = root.join("config.toml");

    let id = std::process::id();
    let mailbox = format!("Scope{id}");
    let marker = |name: &str| format!("scope-{id}-{name}");
    let recent = (Utc::now() - Days::days(1)).to_rfc2822();

    imap(&format!("CREATE {mailbox}"));
    append(
        root,
        &mailbox,
        &marker("old"),
        Some("Wed, 01 Jan 2020 10:00:00 +0000"),
    );
    append(root, &mailbox, &marker("recent"), Some(&recent));
    append(
        root,
        &mailbox,
        &marker("future"),
        Some("Tue, 01 Jan 2030 10:00:00 +0000"),
    );
    append(root, &mailbox, &marker("undated"), None);

    write_config(&config, &store, "scope", &mailbox, Some("30d"));
    neverest(&["init", "-a", "scope"], &config);

    let collection = format!("imap/{mailbox}");

    let report = sync(&config, "scope", &[]);
    assert_eq!(
        stored(&store, &collection),
        sorted(&[marker("future"), marker("recent"), marker("undated")]),
        "the Date decides, not the arrival",
    );
    let coverage = coverage_of(&report, &mailbox);
    let floor = coverage["since"].as_str().expect("a scoped coverage");
    assert!(floor.ends_with("T00:00:00Z"), "the floor is a day: {floor}");
    assert!(coverage["at"].is_string(), "the round closed");
    assert!(coverage.get("round").is_none());
    assert!(
        report["downloaded"]
            .as_array()
            .is_some_and(|sources| sources.iter().any(|s| s["source"] == "imap")),
        "the source counts what it received: {report}",
    );

    let report = sync(&config, "scope", &["--since", "2019-01-01"]);
    assert_eq!(
        stored(&store, &collection),
        sorted(&[
            marker("future"),
            marker("old"),
            marker("recent"),
            marker("undated"),
        ]),
        "a wider scope brings what it reaches",
    );
    assert_eq!(
        coverage_of(&report, &mailbox)["since"],
        "2019-01-01T00:00:00Z"
    );

    sync(&config, "scope", &[]);
    assert!(
        stored(&store, &collection).contains(&marker("old")),
        "a narrower scope deletes nothing from the store",
    );
    assert_eq!(
        server_seqs(&mailbox, &marker("old")).len(),
        1,
        "nor from the server"
    );

    let old = server_seqs(&mailbox, &marker("old"));
    imap_in(&mailbox, &format!("STORE {} +FLAGS (\\Deleted)", old[0]));
    imap_in(&mailbox, "EXPUNGE");
    sync(&config, "scope", &[]);
    assert_eq!(
        stored(&store, &collection),
        sorted(&[marker("future"), marker("recent"), marker("undated")]),
        "a removal out of scope still applies",
    );
    for name in ["recent", "future", "undated"] {
        assert_eq!(
            server_seqs(&mailbox, &marker(name)).len(),
            1,
            "{name} is untouched on the server"
        );
    }

    imap(&format!("DELETE {mailbox}"));
}

/// Three pages of 500 UIDs, the run killed once its first page landed.
const PAGED: usize = 1_100;

#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart.sh) on :143 and --ignored"]
fn an_interrupted_round_resumes_and_lands_every_message_once() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let store = root.join("store");
    let config = root.join("config.toml");

    let id = std::process::id();
    let mailbox = format!("Paged{id}");
    imap(&format!("CREATE {mailbox}"));
    let date = Utc::now().to_rfc2822();
    for n in 0..PAGED {
        append(root, &mailbox, &format!("paged-{id}-{n}"), Some(&date));
    }

    write_config(&config, &store, "paged", &mailbox, None);
    neverest(&["init", "-a", "paged"], &config);
    let collection = format!("imap/{mailbox}");

    let mut first = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(["sync", "-a", "paged", "-j", "1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn neverest");
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut interrupted = false;
    loop {
        if first.try_wait().unwrap().is_some() {
            break;
        }
        if let Some((count, open)) = round_state(&store, &collection)
            && open
            && count > 0
        {
            first.kill().unwrap();
            interrupted = true;
            break;
        }
        assert!(Instant::now() < deadline, "the round never landed a page");
        thread::sleep(Duration::from_millis(2));
    }
    first.wait().unwrap();

    if interrupted {
        let (count, open) = round_state(&store, &collection).unwrap();
        assert!(open, "the round is left open");
        assert!(count < PAGED, "the run was stopped mid-round ({count})");
    } else {
        eprintln!("the first run ended before a page could be caught open");
    }

    let report = sync(&config, "paged", &[]);
    let coverage = coverage_of(&report, &mailbox);
    assert!(coverage["at"].is_string(), "the round closed: {coverage}");
    assert!(coverage.get("round").is_none());
    assert_eq!(
        round_state(&store, &collection),
        Some((PAGED, false)),
        "every message landed once"
    );
    assert_eq!(stored(&store, &collection).len(), PAGED);

    imap(&format!("DELETE {mailbox}"));
}

fn write_config(config: &Path, store: &Path, account: &str, mailbox: &str, since: Option<&str>) {
    let since = since
        .map(|since| format!("item.filter.since = \"{since}\"\n"))
        .unwrap_or_default();
    fs::write(
        config,
        format!(
            "[accounts.{account}]\n\
             store.root = \"{}\"\n\
             imap.server = \"{SERVER}\"\n\
             imap.starttls = false\n\
             imap.sasl.plain.username = \"test@pimalaya.org\"\n\
             imap.sasl.plain.password.raw = \"P!malaya-test-2026\"\n\
             imap.collection.filter.include = [\"{mailbox}\"]\n\
             {since}",
            store.display(),
        ),
    )
    .unwrap();
}

/// Runs a sync, answering its JSON report.
fn sync(config: &Path, account: &str, extra: &[&str]) -> serde_json::Value {
    let mut args = vec!["--json", "sync", "-a", account];
    args.extend_from_slice(extra);
    let out = neverest(&args, config);
    serde_json::from_str(&out).expect("a JSON report")
}

/// The coverage the report states for `mailbox`.
fn coverage_of(report: &serde_json::Value, mailbox: &str) -> serde_json::Value {
    report["coverage"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["collection"] == mailbox))
        .cloned()
        .unwrap_or_else(|| panic!("no coverage for {mailbox}: {report}"))
}

/// The markers of the messages the store holds in `collection`.
fn stored(store: &Path, collection: &str) -> Vec<String> {
    let reader = PimdirReader::open(store).unwrap();
    let mut markers: Vec<String> = reader
        .list_summaries(collection, None, 10_000)
        .unwrap()
        .into_iter()
        .filter_map(|item| {
            let hint = item.summary.as_ref()?.hint()?.to_owned();
            Some(hint.trim_end_matches("@pimalaya.org").to_owned())
        })
        .collect();
    markers.sort();
    markers
}

/// How many items the store holds in `collection`, and whether the
/// source's round over it is open; `None` before the store can be read.
fn round_state(store: &Path, collection: &str) -> Option<(usize, bool)> {
    let reader = PimdirReader::open(store).ok()?;
    let count = reader.count_items(collection).ok()? as usize;
    let open = reader
        .list_coverage(collection)
        .ok()?
        .iter()
        .any(|row| row.round.is_some());
    Some((count, open))
}

fn sorted(markers: &[String]) -> Vec<String> {
    let mut markers = markers.to_vec();
    markers.sort();
    markers
}

/// Appends a message whose `Message-ID` and subject carry `marker`, dated
/// `date` when given.
fn append(root: &Path, mailbox: &str, marker: &str, date: Option<&str>) {
    let eml = root.join("msg.eml");
    let date = date
        .map(|date| format!("Date: {date}\r\n"))
        .unwrap_or_default();
    fs::write(
        &eml,
        format!(
            "Message-ID: <{marker}@pimalaya.org>\r\n\
             From: alice@pimalaya.org\r\n\
             To: bob@pimalaya.org\r\n\
             Subject: neverest scope {marker}\r\n\
             {date}\
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

/// The sequence numbers of the messages of `mailbox` carrying `marker`.
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
