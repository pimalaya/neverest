//! `sync --intents-only` and the fate of a queued send, against a scripted
//! SMTP server on a local port and an IMAP source nothing listens for.
//!
//! The IMAP port is closed on purpose: a run that opened the source would
//! name it unreached, so a report without it proves the run sent through
//! its SMTP channel alone and listed nothing. No server, no Docker.
//!
//! ```sh
//! cargo test --test intents
//! ```
//!
//! Every test configures an IMAP source sending through an `smtp` table, so
//! a build without either has nothing here to run.

#![cfg(all(feature = "imap", feature = "smtp"))]

use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Output},
    sync::mpsc,
    thread,
};

use io_pimdir::{
    client::{
        PimdirStore,
        producer::{PimdirActionStatus, PimdirProducer},
        reader::PimdirReader,
    },
    collection::PimdirCollectionId,
    intent::PimdirSubmit,
};
use serde_json::Value;

const ACCOUNT: &str = "send";
/// The account's IMAP source, as the store keys it.
const SOURCE: &str = "send/imap";
const SENT: &str = "send/imap/Sent";
const SUBJECT: &str = "intents-only";

/// A local port nothing listens on.
fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// How the scripted SMTP server answers.
#[derive(Clone, Copy)]
enum Smtp {
    /// Takes the message.
    Accept,
    /// Reads the whole message, then hangs up without a verdict.
    HangUpAfterData,
    /// Reads the whole message, then refuses it with this reply.
    RefuseData(&'static str),
    /// Refuses the credentials.
    RefuseAuth,
}

/// Serves one SMTP session as `smtp` says, sending back the message data
/// it read.
fn spawn_smtp(smtp: Smtp) -> (u16, mpsc::Receiver<Vec<u8>>) {
    spawn_gated_smtp(smtp, None)
}

/// The same, holding its verdict once the data is read: it says so on the
/// first channel of `gate` and answers once told to on the second.
fn spawn_gated_smtp(
    smtp: Smtp,
    gate: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
) -> (u16, mpsc::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut stream = stream;
        let mut data = Vec::new();
        stream.write_all(b"220 sink\r\n").unwrap();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let upper = line.to_ascii_uppercase();
            if upper.starts_with("EHLO") {
                stream.write_all(b"250-sink\r\n250 AUTH PLAIN\r\n").unwrap();
            } else if upper.starts_with("AUTH") {
                stream
                    .write_all(b"535 5.7.8 authentication failed\r\n")
                    .unwrap();
                break;
            } else if upper.starts_with("DATA") {
                stream.write_all(b"354 go\r\n").unwrap();
                loop {
                    let mut line = Vec::new();
                    if reader.read_until(b'\n', &mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    if line == b".\r\n" {
                        break;
                    }
                    data.extend_from_slice(&line);
                }
                if let Some((read, answer)) = &gate {
                    read.send(()).unwrap();
                    answer.recv().unwrap();
                }
                let reply = match smtp {
                    Smtp::HangUpAfterData => break,
                    Smtp::RefuseData(reply) => reply,
                    _ => "250 queued\r\n",
                };
                let _ = stream.write_all(reply.as_bytes());
            } else if upper.starts_with("QUIT") {
                let _ = stream.write_all(b"221 bye\r\n");
                break;
            } else {
                stream.write_all(b"250 OK\r\n").unwrap();
            }
        }
        let _ = tx.send(data);
    });

    (port, rx)
}

struct Fixture {
    _tmp: tempfile::TempDir,
    config: PathBuf,
    state: PathBuf,
}

impl Fixture {
    /// An account whose IMAP source nothing listens for, sending through
    /// `smtp_port`, authenticated when `auth` says so; its store holds
    /// `imap/Sent` and nothing else.
    fn new(smtp_port: u16, auth: bool) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let config = tmp.path().join("config.toml");
        fs::create_dir_all(&state).unwrap();
        let sasl = match auth {
            true => {
                "smtp.sasl.plain.username = \"user\"\nsmtp.sasl.plain.password.raw = \"secret\"\n"
            }
            false => "",
        };
        fs::write(
            &config,
            format!(
                "[accounts.{ACCOUNT}]\n\
                 imap.server = \"imap://127.0.0.1:{}\"\n\
                 imap.starttls = false\n\
                 imap.sasl.plain.username = \"user\"\n\
                 imap.sasl.plain.password.raw = \"secret\"\n\
                 smtp.server = \"smtp://127.0.0.1:{smtp_port}\"\n\
                 smtp.starttls = false\n\
                 {sasl}",
                closed_port(),
            ),
        )
        .unwrap();

        let fixture = Self {
            _tmp: tmp,
            config,
            state,
        };

        // NOTE: the store and its sidecar naming the account, as `init`
        // leaves them, then the collection.
        let local = PimdirStore::open(fixture.store())
            .unwrap()
            .local_dir()
            .unwrap();
        let sidecar = format!(r#"{{"layout":2,"accounts":{{"{ACCOUNT}":{{}}}}}}"#);
        fs::write(local.join("neverest.json"), sidecar).unwrap();
        PimdirStore::open(fixture.store())
            .unwrap()
            .for_account(ACCOUNT)
            .for_source(SOURCE)
            .ensure_collection(SENT, "message/rfc822")
            .unwrap();

        fixture
    }

    fn store(&self) -> PathBuf {
        self.state.join("neverest")
    }

    /// The store's local/, where neverest keeps its own state.
    fn local(&self) -> PathBuf {
        self.store().join("local")
    }

    /// Queues a send of `body`, as `himalaya pimdir message send` does,
    /// its copy filed in Sent when `copy` says so; returns the queue row.
    fn queue_send(&self, body: &[u8], copy: bool) -> i64 {
        let submit = PimdirSubmit {
            source: String::from(SOURCE),
            from: String::from("a@example.org"),
            rcpts: vec![String::from("b@example.org")],
            subject: Some(String::from(SUBJECT)),
            copy: copy.then(|| PimdirCollectionId::from(SENT)),
        };
        PimdirProducer::open(self.store(), "neverest-tests")
            .unwrap()
            .enqueue_with_body(SENT, body, |object| submit.to_action(object).unwrap())
            .unwrap()
            .handle
            .id
    }

    fn status(&self, id: i64) -> PimdirActionStatus {
        PimdirProducer::open(self.store(), "neverest-tests")
            .unwrap()
            .action_status(id)
            .unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_neverest"))
            .args(["-c", &self.config.to_string_lossy()])
            .args(args)
            .env("XDG_STATE_HOME", &self.state)
            .output()
            .expect("spawn neverest")
    }

    /// The report of `sync --intents-only --json`, exiting `code`.
    fn intents_only(&self, code: i32) -> Value {
        let output = self.run(&["--json", "sync", "-a", ACCOUNT, "--intents-only"]);
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            stderr(&output),
        );
        // NOTE: the last line, after the events a sync prints.
        let stdout = String::from_utf8_lossy(&output.stdout);
        let result: Value =
            serde_json::from_str(stdout.lines().last().unwrap_or_default()).expect("a JSON result");
        assert_eq!(result["accounts"][0]["account"], ACCOUNT, "{result}");
        result["accounts"][0]["report"].clone()
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

const BODY: &[u8] =
    b"From: a@example.org\r\nTo: b@example.org\r\nSubject: intents-only\r\n\r\nhello\r\n";

/// The message leaves through the SMTP channel alone: the source, whose
/// server is down, is never opened, and no collection is listed.
#[test]
fn intents_only_sends_through_smtp_without_opening_the_source() {
    let (port, data) = spawn_smtp(Smtp::Accept);
    let fixture = Fixture::new(port, false);
    let id = fixture.queue_send(BODY, true);

    let report = fixture.intents_only(0);

    assert_eq!(data.recv().unwrap(), BODY, "{report}");
    let submitted = &report["submitted"][0];
    assert_eq!(submitted["id"], id, "{report}");
    assert_eq!(submitted["error"], Value::Null, "{report}");
    assert_eq!(submitted["parked"], false, "{report}");
    assert_eq!(submitted["unconfirmed"], false, "{report}");
    assert_eq!(report["unreached"], serde_json::json!([]), "{report}");
    assert_eq!(
        report["collection"]["patch"],
        serde_json::json!([]),
        "{report}"
    );
    assert!(matches!(
        fixture.status(id),
        PimdirActionStatus::Applied { seq: None, .. }
    ));
    // NOTE: the copy is in the store at once, pushed by the next sync.
    assert_eq!(report["drained"][0]["collection"], SENT, "{report}");
    let held = PimdirReader::open(fixture.store())
        .unwrap()
        .list_summaries(SENT, None, 10)
        .unwrap();
    assert_eq!(held.len(), 1, "{report}");
}

/// A message read whole by a server that hung up before its verdict may
/// have been delivered: parked unconfirmed, under `unconfirmed: `, never
/// sent again.
#[test]
fn a_send_without_a_verdict_is_parked_as_unconfirmed() {
    let (port, data) = spawn_smtp(Smtp::HangUpAfterData);
    let fixture = Fixture::new(port, false);
    let id = fixture.queue_send(BODY, false);

    let report = fixture.intents_only(3);

    assert_eq!(data.recv().unwrap(), BODY);
    let submitted = &report["submitted"][0];
    assert_eq!(submitted["parked"], true, "{report}");
    assert_eq!(submitted["unconfirmed"], true, "{report}");
    let error = submitted["error"].as_str().unwrap();
    assert!(error.starts_with("unconfirmed: "), "{report}");
    let PimdirActionStatus::Unconfirmed { detail, .. } = fixture.status(id) else {
        panic!("the send is parked unconfirmed: {report}");
    };
    assert_eq!(error, format!("unconfirmed: {detail}"));

    // NOTE: the next run has nothing to send, the server being gone, and
    // still exits 3 over the parked send waiting for a person.
    let report = fixture.intents_only(3);
    assert_eq!(report["submitted"], serde_json::json!([]), "{report}");
    assert_eq!(report["unreached"], serde_json::json!([]), "{report}");
    assert_eq!(report["parked"][0]["id"], id, "{report}");
}

/// A run stopped mid-send leaves its mark: the next one parks that send
/// as unconfirmed rather than sending it again.
#[test]
fn a_send_a_stopped_run_was_handing_over_is_never_sent_again() {
    // NOTE: nothing listens: a run trying to send would name it unreached.
    let fixture = Fixture::new(closed_port(), false);
    let id = fixture.queue_send(BODY, false);
    fs::write(fixture.local().join("submitting"), format!("{id}\n")).unwrap();

    let report = fixture.intents_only(3);

    let submitted = &report["submitted"][0];
    assert_eq!(submitted["id"], id, "{report}");
    assert_eq!(submitted["parked"], true, "{report}");
    assert_eq!(submitted["unconfirmed"], true, "{report}");
    assert_eq!(report["unreached"], serde_json::json!([]), "{report}");
    assert!(matches!(
        fixture.status(id),
        PimdirActionStatus::Unconfirmed { .. }
    ));
    assert!(!fixture.local().join("submitting").exists());
}

/// An SMTP server that cannot be reached is named, and the send waits,
/// never attempted.
#[test]
fn an_unreached_smtp_server_is_named_and_the_send_waits() {
    let fixture = Fixture::new(closed_port(), false);
    let id = fixture.queue_send(BODY, false);

    let report = fixture.intents_only(4);

    let unreached = report["unreached"].as_array().expect("unreached");
    assert_eq!(unreached.len(), 1, "{report}");
    assert_eq!(unreached[0]["endpoint"], "imap.smtp", "{report}");
    assert_eq!(report["submitted"], serde_json::json!([]), "{report}");
    assert!(matches!(
        fixture.status(id),
        PimdirActionStatus::Pending { attempts: 0, .. }
    ));
}

/// Refused credentials are a server answering: each send fails with the
/// reason, not parked and never attempted, and nothing is unreached.
#[test]
fn refused_credentials_fail_the_send_without_parking_it() {
    let (port, _) = spawn_smtp(Smtp::RefuseAuth);
    let fixture = Fixture::new(port, true);
    let id = fixture.queue_send(BODY, false);

    let report = fixture.intents_only(4);

    assert_eq!(report["unreached"], serde_json::json!([]), "{report}");
    let submitted = &report["submitted"][0];
    assert!(
        submitted["error"].as_str().unwrap().contains("535"),
        "{report}"
    );
    assert_eq!(submitted["parked"], false, "{report}");
    assert_eq!(submitted["unconfirmed"], false, "{report}");
    assert!(matches!(
        fixture.status(id),
        PimdirActionStatus::Pending { attempts: 0, .. }
    ));
}

/// A send refused for a daily limit waits for the next run, which the run's
/// other sends wait for too: the limit holds for the account.
#[test]
fn a_send_refused_for_a_quota_stays_queued() {
    let reply = "550 5.4.5 Daily user sending limit exceeded\r\n";
    let (port, data) = spawn_smtp(Smtp::RefuseData(reply));
    let fixture = Fixture::new(port, false);
    let first = fixture.queue_send(BODY, false);
    let second = fixture.queue_send(BODY, false);

    let report = fixture.intents_only(4);

    assert_eq!(data.recv().unwrap(), BODY, "{report}");
    let submitted = &report["submitted"];
    assert_eq!(submitted[0]["id"], first, "{report}");
    let error = submitted[0]["error"].as_str().unwrap();
    assert!(error.contains("5.4.5"), "{report}");
    assert_eq!(submitted[0]["parked"], false, "{report}");
    assert_eq!(submitted[1]["id"], second, "{report}");
    let error = submitted[1]["error"].as_str().unwrap();
    assert!(error.starts_with("Not attempted"), "{report}");
    assert_eq!(submitted[1]["parked"], false, "{report}");
    assert!(matches!(
        fixture.status(first),
        PimdirActionStatus::Pending { attempts: 1, .. }
    ));
    assert!(matches!(
        fixture.status(second),
        PimdirActionStatus::Pending { attempts: 0, .. }
    ));
}

/// A collection filter means nothing to a run listing no collection.
#[test]
fn intents_only_refuses_a_collection_filter() {
    let fixture = Fixture::new(closed_port(), false);
    let output = fixture.run(&["sync", "-a", ACCOUNT, "--intents-only", "-m", "Sent"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("cannot be used with"),
        "{}",
        stderr(&output)
    );
}

/// While the server holds its verdict, the attempt is already counted and
/// the send marked in flight: a caller killing the run then reads the row
/// as attempted, and the next run never sends it again.
#[test]
fn the_attempt_is_counted_and_marked_before_the_message_leaves() {
    let (read_tx, read_rx) = mpsc::channel();
    let (answer_tx, answer_rx) = mpsc::channel();
    let (port, _) = spawn_gated_smtp(Smtp::Accept, Some((read_tx, answer_rx)));
    let fixture = Fixture::new(port, false);
    let id = fixture.queue_send(BODY, false);

    let run = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &fixture.config.to_string_lossy()])
        .args(["--json", "sync", "-a", ACCOUNT, "--intents-only"])
        .env("XDG_STATE_HOME", &fixture.state)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();

    read_rx.recv().unwrap();
    let in_flight = fs::read_to_string(fixture.local().join("submitting")).unwrap();
    assert_eq!(in_flight.trim(), id.to_string());
    assert!(matches!(
        fixture.status(id),
        PimdirActionStatus::Pending { attempts: 1, .. }
    ));
    answer_tx.send(()).unwrap();

    let output = run.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(!fixture.local().join("submitting").exists());
    assert!(matches!(
        fixture.status(id),
        PimdirActionStatus::Applied { .. }
    ));
}

/// A run killed while the server holds its verdict, as a caller does at
/// its deadline, leaves the row attempted and marked: the next run parks
/// it as unconfirmed and sends nothing.
#[test]
fn a_run_killed_mid_send_never_sends_the_message_again() {
    let (read_tx, read_rx) = mpsc::channel();
    let (answer_tx, answer_rx) = mpsc::channel();
    let (port, _) = spawn_gated_smtp(Smtp::Accept, Some((read_tx, answer_rx)));
    let fixture = Fixture::new(port, false);
    let id = fixture.queue_send(BODY, false);

    let mut run = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &fixture.config.to_string_lossy()])
        .args(["--json", "sync", "-a", ACCOUNT, "--intents-only"])
        .env("XDG_STATE_HOME", &fixture.state)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    read_rx.recv().unwrap();
    run.kill().unwrap();
    run.wait().unwrap();
    answer_tx.send(()).unwrap();

    assert!(matches!(
        fixture.status(id),
        PimdirActionStatus::Pending { attempts: 1, .. }
    ));

    let report = fixture.intents_only(3);
    let submitted = &report["submitted"][0];
    assert_eq!(submitted["id"], id, "{report}");
    assert_eq!(submitted["unconfirmed"], true, "{report}");
    assert_eq!(report["unreached"], serde_json::json!([]), "{report}");
    assert!(matches!(
        fixture.status(id),
        PimdirActionStatus::Unconfirmed { .. }
    ));
}
