//! End-to-end heads of an open IMAP round against a local Stalwart server
//! (:143), spawned via `tests/stalwart2.sh` (or `tests/stalwart.sh`).
//! Ignored by default.
//!
//! A round left open by a run killed after its first page is resumed by
//! the next run, which lists the round's head first (pimdir SYNC §4, §5):
//! what arrived and what was removed meanwhile shows at once, before any
//! further page.
//!
//!   1. With QRESYNC, a delta head from the round's checkpoint lands a
//!      message appended after the first page and drops one expunged from
//!      it, the round still open; the round then closes keeping the
//!      arrival, and the next delta lists nothing new.
//!   2. Read as a server without QRESYNC, a band head (`SEARCH SINCE`)
//!      lands the arrival, the round still open; the round's last page
//!      lists it again, so it survives the close, and the expunged message,
//!      stamped by the first page, goes with the next round.
//!
//! ```sh
//! ./tests/stalwart2.sh
//! cargo test --test heads -- --ignored --test-threads=1
//! ```

use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use chrono::Utc;
use io_pimdir::client::reader::PimdirReader;

const SERVER: &str = "imap://127.0.0.1:143";
const CRED: &str = "test@pimalaya.org:P!malaya-test-2026";

/// Three pages of 500 UIDs, the first run killed once its first page landed.
const PAGED: usize = 1_100;

/// The variable a debug build reads to take the server for one without
/// QRESYNC.
const NO_QRESYNC: &str = "NEVEREST_IMAP_NO_QRESYNC";

#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :143 and --ignored"]
fn a_delta_head_shows_what_changed_while_a_round_is_open() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let open = Opened::new(tmp.path(), "delta", false);

    let report = sync(&open.config, open.account, &open.env, &[]);
    let coverage = coverage_of(&report, &open.mailbox);
    assert!(coverage["at"].is_string(), "the round closed: {coverage}");
    assert!(coverage.get("round").is_none());
    let markers = stored(&open.store, &open.collection);
    assert!(
        markers.contains(&open.arrival),
        "the arrival survives the close"
    );
    assert!(
        !markers.contains(&open.expunged),
        "what the head stated removed stays removed"
    );
    assert_eq!(markers.len(), PAGED, "every message once");

    let report = sync(&open.config, open.account, &open.env, &[]);
    assert_eq!(fetches(&report), 0, "the next delta lists nothing new");
    assert_eq!(stored(&open.store, &open.collection), markers);
}

#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :143 and --ignored"]
fn a_band_head_shows_what_arrived_while_a_round_is_open() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let open = Opened::new(tmp.path(), "band", true);

    // NOTE: a band head stamps nothing; the round's last page listing the
    // arrival again is what keeps it from being found absent at the close.
    // A sync's push pass would list it back at once, so a dry run, one pass
    // naming every removal, is what tells.
    let plan = sync(&open.config, open.account, &open.env, &["--dry-run"]);
    assert_eq!(
        deletes(&plan),
        0,
        "the close finds nothing absent, the arrival listed again: {plan}"
    );

    let report = sync(&open.config, open.account, &open.env, &[]);
    let coverage = coverage_of(&report, &open.mailbox);
    assert!(coverage["at"].is_string(), "the round closed: {coverage}");
    assert!(coverage.get("round").is_none());
    let markers = stored(&open.store, &open.collection);
    assert!(
        markers.contains(&open.arrival),
        "the round's last page keeps what the band head named"
    );

    // NOTE: without QRESYNC every run is a round, whose close finds the
    // expunged message absent.
    sync(&open.config, open.account, &open.env, &[]);
    let markers = stored(&open.store, &open.collection);
    assert!(markers.contains(&open.arrival));
    assert!(!markers.contains(&open.expunged), "the next round drops it");
    assert_eq!(markers.len(), PAGED, "every message once");
}

/// A round left open after its first page, a message appended and one of
/// the first page expunged since, and the resumed run killed as soon as
/// the arrival landed.
struct Opened {
    config: std::path::PathBuf,
    store: std::path::PathBuf,
    account: &'static str,
    mailbox: String,
    collection: String,
    env: Vec<(&'static str, &'static str)>,
    arrival: String,
    expunged: String,
    _dropped: Dropped,
}

impl Opened {
    fn new(root: &Path, account: &'static str, band: bool) -> Self {
        let store = root.join("store");
        let config = root.join("config.toml");
        let env = if band {
            vec![(NO_QRESYNC, "1")]
        } else {
            Vec::new()
        };

        let id = std::process::id();
        let mailbox = format!("Heads{account}{id}");
        let marker = |name: &str| format!("heads-{account}-{id}-{name}");
        imap(&format!("CREATE {mailbox}"));
        let dropped = Dropped(mailbox.clone());
        let date = Utc::now().to_rfc2822();
        for n in 0..PAGED {
            append(root, &mailbox, &marker(&n.to_string()), Some(&date));
        }

        write_config(&config, &store, account, &mailbox);
        neverest(&["init", "-a", account], &config, &env);
        let collection = format!("imap/{mailbox}");

        let listed = run_until(&config, account, &env, || {
            round_state(&store, &collection).is_some_and(|(count, open)| open && count > 0)
        });
        let (_, open) = round_state(&store, &collection).unwrap();
        assert!(listed && open, "the first run was stopped mid-round");
        let before = stored(&store, &collection);
        assert!(
            before.len() < PAGED,
            "the first run listed one page ({})",
            before.len()
        );

        // NOTE: the newest message, which the first page listed.
        let expunged = marker(&(PAGED - 1).to_string());
        assert!(before.contains(&expunged));
        let seqs = server_seqs(&mailbox, &expunged);
        assert_eq!(seqs.len(), 1);
        imap_in(&mailbox, &format!("STORE {} +FLAGS (\\Deleted)", seqs[0]));
        imap_in(&mailbox, "EXPUNGE");

        let arrival = marker("arrival");
        append(root, &mailbox, &arrival, Some(&Utc::now().to_rfc2822()));

        let landed = run_until(&config, account, &env, || {
            stored(&store, &collection).contains(&arrival)
        });
        assert!(landed, "the resumed run landed the arrival before it ended");
        let (_, open) = round_state(&store, &collection).unwrap();
        assert!(open, "the arrival landed while the round was open");
        let after = stored(&store, &collection);
        if band {
            // NOTE: a band head by arrival lists every message appended
            // since the round opened less two days, here all of them, and
            // states no removal.
            assert!(after.len() <= PAGED + 1, "each message once");
        } else {
            assert!(!after.contains(&expunged), "the head stated the expunge");
            assert_eq!(
                after.len(),
                before.len(),
                "one arrived, one left, and no page listed"
            );
        }

        Self {
            config,
            store,
            account,
            mailbox,
            collection,
            env,
            arrival,
            expunged,
            _dropped: dropped,
        }
    }
}

/// Spawns a sync over one connection and kills it once `done` holds,
/// answering whether it did before the run ended.
fn run_until(
    config: &Path,
    account: &str,
    env: &[(&str, &str)],
    mut done: impl FnMut() -> bool,
) -> bool {
    let mut run = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(["sync", "-a", account, "-j", "1"])
        .envs(env.iter().copied())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn neverest");
    let deadline = Instant::now() + Duration::from_secs(120);
    let caught = loop {
        if run.try_wait().unwrap().is_some() {
            break false;
        }
        if done() {
            run.kill().unwrap();
            break true;
        }
        assert!(Instant::now() < deadline, "the run never got there");
        thread::sleep(Duration::from_millis(2));
    };
    run.wait().unwrap();
    caught
}

fn write_config(config: &Path, store: &Path, account: &str, mailbox: &str) {
    fs::write(
        config,
        format!(
            "[accounts.{account}]\n\
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
}

/// Runs a sync, answering its JSON report.
fn sync(config: &Path, account: &str, env: &[(&str, &str)], extra: &[&str]) -> serde_json::Value {
    let mut args = vec!["--json", "sync", "-a", account];
    args.extend_from_slice(extra);
    let out = neverest(&args, config, env);
    serde_json::from_str(&out).expect("a JSON report")
}

/// How many bodies a report fetched.
fn fetches(report: &serde_json::Value) -> usize {
    report["item"]["patch"]
        .as_array()
        .expect("item patch")
        .iter()
        .filter(|entry| entry["hunk"]["kind"] == "fetch")
        .count()
}

/// How many removals a report names.
fn deletes(report: &serde_json::Value) -> usize {
    report["item"]["patch"]
        .as_array()
        .expect("item patch")
        .iter()
        .filter(|entry| entry["hunk"]["kind"] == "delete")
        .count()
}

/// The coverage the report states for `mailbox`.
fn coverage_of(report: &serde_json::Value, mailbox: &str) -> serde_json::Value {
    report["coverage"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["collection"] == mailbox))
        .cloned()
        .unwrap_or_else(|| panic!("no coverage for {mailbox}: {report}"))
}

/// The markers of the messages the store holds in `collection`, none
/// before the store can be read.
fn stored(store: &Path, collection: &str) -> Vec<String> {
    let Ok(reader) = PimdirReader::open(store) else {
        return Vec::new();
    };
    let mut markers: Vec<String> = reader
        .list_summaries(collection, None, 10_000)
        .unwrap_or_default()
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
             Subject: neverest heads {marker}\r\n\
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
fn neverest(args: &[&str], config: &Path, env: &[(&str, &str)]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(args)
        .envs(env.iter().copied())
        .output()
        .expect("spawn neverest");
    assert!(
        output.status.success() || output.status.code() == Some(3),
        "`neverest {}` failed:\n--- stdout ---\n{}\n--- stderr ---\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Deletes its mailbox when dropped, the test passing or not.
struct Dropped(String);

impl Drop for Dropped {
    fn drop(&mut self) {
        let _ = Command::new("curl")
            .args(["-fsS", "--user", CRED, SERVER, "-X"])
            .arg(format!("DELETE {}", self.0))
            .output();
    }
}
