//! What a frontend queued is applied before any endpoint is opened, by
//! `neverest drain` or at the start of a sync, so the frontend reads its
//! own writes back offline.
//!
//! The first tests need no server: the CalDAV endpoint is a closed port.
//! The last one pushes the drained event to a local Stalwart (HTTP and
//! DAV on :8080, `tests/stalwart2.sh`) and is ignored by default:
//! ```sh
//! ./tests/stalwart2.sh
//! cargo test --test drain -- --ignored --test-threads=1
//! ```

use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use io_pimdir::{
    client::{
        PimdirStore,
        producer::{PimdirActionStatus, PimdirProducer},
        reader::PimdirReader,
    },
    codec::PimdirAction,
    object::PimdirObject,
    placement::{PimdirFlags, PimdirLinkId},
};
use serde_json::Value;

const ACCOUNT: &str = "drain";
const COLLECTION: &str = "caldav/default";
/// A port nothing listens on, so a run that connects fails.
const CLOSED: &str = "http://127.0.0.1:1/";
const DAV: &str = "http://127.0.0.1:8080/dav";
const USER: &str = "test@pimalaya.org";
const PASS: &str = "P!malaya-test-2026";

fn event(uid: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\n\
         VERSION:2.0\r\n\
         PRODID:-//pimalaya//neverest tests//EN\r\n\
         BEGIN:VEVENT\r\n\
         UID:{uid}\r\n\
         DTSTAMP:20261006T080000Z\r\n\
         DTSTART:20261006T090000Z\r\n\
         DTEND:20261006T100000Z\r\n\
         SUMMARY:Drained\r\n\
         END:VEVENT\r\n\
         END:VCALENDAR\r\n",
    )
}

/// A UID no earlier run left on the server.
fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after the epoch")
        .as_nanos();
    format!("{prefix}-{nanos}")
}

struct Fixture {
    _tmp: tempfile::TempDir,
    config: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new(server: &str, user: &str, pass: &str) -> Self {
        let tmp = tempfile::tempdir().expect("temp dir");
        let state = tmp.path().join("state");
        let config = tmp.path().join("config.toml");
        fs::create_dir_all(&state).unwrap();
        let fixture = Self {
            _tmp: tmp,
            config,
            state,
        };
        fixture.point_at(server, user, pass);
        fixture
    }

    fn point_at(&self, server: &str, user: &str, pass: &str) {
        fs::write(
            &self.config,
            format!(
                "[accounts.{ACCOUNT}]\n\
                 caldav.server = \"{server}\"\n\
                 caldav.auth.basic.username = \"{user}\"\n\
                 caldav.auth.basic.password.raw = \"{pass}\"\n",
            ),
        )
        .unwrap();
    }

    fn store(&self) -> PathBuf {
        self.state.join("neverest").join(ACCOUNT)
    }

    /// A store holding the calendar, made without any server: the
    /// database first, then a reset, which stamps it and fails to
    /// connect, then the collection.
    fn offline_store(&self) {
        fs::create_dir_all(self.store()).unwrap();
        drop(PimdirStore::open(self.store()).unwrap());
        let reset = self.run(&["sync", "-a", ACCOUNT, "--reset", "--json"]);
        assert_eq!(reset.status.code(), Some(3), "{}", stderr(&reset));

        let store = PimdirStore::open(self.store())
            .unwrap()
            .for_account(ACCOUNT)
            .for_source("caldav");
        store
            .ensure_collection(COLLECTION, "text/calendar")
            .unwrap();
    }

    /// Stages an event's body and queues its creation, as calendula does;
    /// returns the queue row.
    fn queue_event(&self, uid: &str) -> i64 {
        let body = event(uid);
        let reader = PimdirReader::open(self.store()).unwrap();
        let hash = reader.hash(body.as_bytes());
        let mut writer = reader.blobs().writer().unwrap();
        writer.write_all(body.as_bytes()).unwrap();
        let size = writer.commit(&hash).unwrap();

        let mut producer = PimdirProducer::open(self.store(), "neverest-tests").unwrap();
        producer
            .enqueue(
                COLLECTION,
                &PimdirAction::Add {
                    link_id: Some(PimdirLinkId(uid.into())),
                    flags: PimdirFlags::default(),
                    object: Some(hash.clone()),
                },
                Some(&PimdirObject {
                    hash,
                    size: size as usize,
                }),
            )
            .unwrap()
    }

    fn status(&self, id: i64) -> PimdirActionStatus {
        PimdirProducer::open(self.store(), "neverest-tests")
            .unwrap()
            .action_status(id)
            .unwrap()
    }

    /// The items the collection lists under `uid`, by their public id.
    fn seqs_of(&self, uid: &str) -> Vec<i64> {
        PimdirReader::open(self.store())
            .unwrap()
            .list_summaries(COLLECTION, None, 1000)
            .unwrap()
            .into_iter()
            .filter(|item| item.link_id.0 == uid)
            .map(|item| item.seq)
            .collect()
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_neverest"))
            .args(["-c", &self.config.to_string_lossy()])
            .args(args)
            .env("XDG_STATE_HOME", &self.state)
            .output()
            .expect("spawn neverest")
    }

    fn json(&self, args: &[&str], code: i32) -> Value {
        let output = self.run(args);
        assert_eq!(
            output.status.code(),
            Some(code),
            "`neverest {}`:\n{}\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            stderr(&output),
        );
        serde_json::from_slice(&output.stdout).expect("one JSON document")
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// `neverest drain` gives a queued create its id, offline, and says which.
#[test]
fn a_drain_applies_a_queued_create_without_any_server() {
    let fixture = Fixture::new(CLOSED, "user", "pass");
    fixture.offline_store();
    let id = fixture.queue_event("drained-1");
    assert!(fixture.seqs_of("drained-1").is_empty());

    let drained = fixture.json(&["--json", "drain", "-a", ACCOUNT], 0);

    assert_eq!(drained["busy"], false, "{drained}");
    let applied = &drained["applied"][0];
    assert_eq!(applied["id"], id, "{drained}");
    assert_eq!(applied["kind"], "add", "{drained}");
    assert_eq!(applied["collection"], COLLECTION, "{drained}");
    let seq = applied["seq"].as_i64().expect("the created item's id");
    assert_eq!(fixture.seqs_of("drained-1"), vec![seq]);
}

/// A sync that cannot reach its server still applies the queue first.
#[test]
fn a_sync_drains_before_it_connects() {
    let fixture = Fixture::new(CLOSED, "user", "pass");
    fixture.offline_store();
    let id = fixture.queue_event("drained-2");

    let report = fixture.json(&["--json", "sync", "-a", ACCOUNT], 3);

    assert_eq!(report["drained"][0]["collection"], COLLECTION, "{report}");
    let PimdirActionStatus::Applied { seq: Some(seq), .. } = fixture.status(id) else {
        panic!("the create is applied: {report}");
    };
    assert_eq!(fixture.seqs_of("drained-2"), vec![seq]);
}

/// A drain never waits for a sync: it says the store is busy, the queue
/// left as it was.
#[test]
fn a_drain_leaves_a_busy_store_alone() {
    let fixture = Fixture::new(CLOSED, "user", "pass");
    fixture.offline_store();
    let id = fixture.queue_event("drained-3");

    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(fixture.store().join("sync.lock"))
        .unwrap();
    lock.lock().unwrap();

    let drained = fixture.json(&["--json", "drain", "-a", ACCOUNT], 3);
    assert_eq!(drained["busy"], true, "{drained}");
    assert!(matches!(
        fixture.status(id),
        PimdirActionStatus::Pending { .. }
    ));

    drop(lock);
    fixture.json(&["--json", "drain", "-a", ACCOUNT], 0);
    assert!(matches!(
        fixture.status(id),
        PimdirActionStatus::Applied { seq: Some(_), .. }
    ));
}

/// The id a drain gives an event is the one it keeps once the sync has
/// pushed it and fetched it back: a frontend showing it at once shows
/// the same item later.
#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :8080 and --ignored"]
fn a_drained_event_keeps_its_id_across_the_push() {
    let server = format!("{DAV}/cal/");
    let fixture = Fixture::new(&server, USER, PASS);
    fixture.json(&["--json", "init", "-a", ACCOUNT], 0);
    fixture.json(&["--json", "sync", "-a", ACCOUNT, "-m", "default"], 0);

    let uid = unique("drained");
    fixture.queue_event(&uid);
    let drained = fixture.json(&["--json", "drain", "-a", ACCOUNT], 0);
    let seq = drained["applied"][0]["seq"]
        .as_i64()
        .expect("the created item's id");

    // NOTE: the first sync pushes it, the second fetches it back.
    fixture.json(&["--json", "sync", "-a", ACCOUNT, "-m", "default"], 0);
    fixture.json(&["--json", "sync", "-a", ACCOUNT, "-m", "default"], 0);
    assert_eq!(fixture.seqs_of(&uid), vec![seq]);

    // A second store of the same server lists it: it was pushed.
    let other = Fixture::new(&server, USER, PASS);
    other.json(&["--json", "init", "-a", ACCOUNT], 0);
    other.json(&["--json", "sync", "-a", ACCOUNT, "-m", "default"], 0);
    assert_eq!(other.seqs_of(&uid).len(), 1);

    remove_event(&uid);
}

fn remove_event(uid: &str) {
    let _ = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-X", "DELETE", "-u"])
        .arg(format!("{USER}:{PASS}"))
        .arg(format!("{DAV}/cal/test%40pimalaya.org/default/{uid}.ics"))
        .output();
}
