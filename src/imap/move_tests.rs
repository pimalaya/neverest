//! # IMAP move dialogues
//!
//! Scripted loopback exchanges check capability selection, failure boundaries
//! and retries on a fresh connection without a real mailbox or credentials.

use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    thread::{self, JoinHandle},
    time::Duration,
};

use pimalaya_stream::tls::Tls;
use url::Url;

use crate::imap::client::ImapClient;

const SELECTED: &str = "* 2 EXISTS\r\n* OK [UIDVALIDITY 1] valid\r\n";

struct Step {
    command: &'static str,
    rows: &'static str,
    completion: Option<&'static str>,
}

impl Step {
    fn ok(command: &'static str, rows: &'static str) -> Self {
        Self {
            command,
            rows,
            completion: Some("OK done"),
        }
    }
}

struct Server {
    url: Url,
    task: JoinHandle<()>,
}

impl Server {
    fn start(capabilities: &'static str, sessions: Vec<Vec<Step>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = Url::parse(&format!("imap://{}", listener.local_addr().unwrap())).unwrap();
        let task = thread::spawn(move || {
            for steps in sessions {
                let (mut writer, _) = listener.accept().unwrap();
                writer
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut reader = BufReader::new(writer.try_clone().unwrap());
                write!(writer, "* PREAUTH [CAPABILITY {capabilities}] ready\r\n").unwrap();
                let mut closed = false;
                for step in steps {
                    let mut line = String::new();
                    assert_ne!(reader.read_line(&mut line).unwrap(), 0, "{}", step.command);
                    let (tag, command) = line.trim_end().split_once(' ').unwrap();
                    assert_eq!(command, step.command);
                    writer.write_all(step.rows.as_bytes()).unwrap();
                    match step.completion {
                        Some(completion) => write!(writer, "{tag} {completion}\r\n").unwrap(),
                        None => {
                            closed = true;
                            break;
                        }
                    }
                }
                if !closed {
                    let mut extra = String::new();
                    assert_eq!(
                        reader.read_line(&mut extra).unwrap(),
                        0,
                        "unexpected: {extra}"
                    );
                }
            }
        });
        Self { url, task }
    }

    fn connect(&self) -> ImapClient {
        ImapClient::connect(&self.url, &Tls::default(), false, None).unwrap()
    }

    fn finish(self) {
        self.task.join().unwrap();
    }
}

fn absent_target() -> Vec<Step> {
    vec![
        Step::ok("SELECT Archive", SELECTED),
        Step::ok("UID SEARCH HEADER Message-ID <a@x>", "* SEARCH\r\n"),
        Step::ok("SELECT INBOX", SELECTED),
    ]
}

#[test]
fn advertised_native_moves_need_no_uidplus_or_searchable_identity() {
    for capabilities in ["IMAP4rev1 MOVE", "IMAP4rev2"] {
        let server = Server::start(
            capabilities,
            vec![vec![
                Step::ok("SELECT INBOX", SELECTED),
                Step::ok("UID MOVE 7 Archive", "* 1 EXPUNGE\r\n"),
            ]],
        );
        let mut client = server.connect();
        client
            .move_messages("INBOX", "Archive", &["7"], None)
            .unwrap();
        drop(client);
        server.finish();
    }
}

#[test]
fn uidplus_without_move_copies_then_expunge_only_the_source_uid() {
    let mut steps = absent_target();
    steps.extend([
        Step::ok("UID COPY 7 Archive", ""),
        Step::ok("UID STORE 7 +FLAGS (\\Deleted)", ""),
        Step::ok("UID EXPUNGE 7", "* 1 EXPUNGE\r\n"),
    ]);
    let server = Server::start("IMAP4rev1 UIDPLUS", vec![steps]);
    let mut client = server.connect();
    client
        .move_messages("INBOX", "Archive", &["7"], Some("a@x"))
        .unwrap();
    drop(client);
    server.finish();
}

#[test]
fn an_unsafe_fallback_is_refused_before_any_command() {
    for (capabilities, ids, identity, target) in [
        ("IMAP4rev1", vec!["7"], Some("a@x"), "Archive"),
        ("IMAP4rev1 UIDPLUS", vec!["7"], None, "Archive"),
        ("IMAP4rev1 UIDPLUS", vec!["7"], Some(""), "Archive"),
        ("IMAP4rev1 UIDPLUS", vec!["7", "8"], Some("a@x"), "Archive"),
        ("IMAP4rev1 UIDPLUS", vec!["7"], Some("a@x"), "INBOX"),
    ] {
        let server = Server::start(capabilities, vec![vec![]]);
        let mut client = server.connect();
        assert!(
            client
                .move_messages("INBOX", target, &ids, identity)
                .is_err()
        );
        drop(client);
        server.finish();
    }
}

#[test]
fn a_failed_search_or_copy_never_marks_or_expunges_the_source() {
    for copy in [false, true] {
        let mut steps = absent_target();
        if copy {
            steps.push(Step {
                command: "UID COPY 7 Archive",
                rows: "",
                completion: Some("NO [OVERQUOTA] full"),
            });
        } else {
            steps.truncate(2);
            steps[1].completion = Some("NO search failed");
        }
        let server = Server::start("IMAP4rev1 UIDPLUS", vec![steps]);
        let mut client = server.connect();
        assert!(
            client
                .move_messages("INBOX", "Archive", &["7"], Some("a@x"))
                .is_err()
        );
        drop(client);
        server.finish();
    }
}

#[test]
fn an_interrupted_fallback_is_finished_without_copying_again_after_reconnect() {
    for failed in ["copy-answer", "store", "expunge"] {
        let mut first = absent_target();
        first.push(Step {
            command: "UID COPY 7 Archive",
            rows: "",
            completion: (failed != "copy-answer").then_some("OK [COPYUID 2 7 81] copied"),
        });
        if failed != "copy-answer" {
            first.push(Step {
                command: "UID STORE 7 +FLAGS (\\Deleted)",
                rows: "",
                completion: Some(if failed == "store" {
                    "NO busy"
                } else {
                    "OK stored"
                }),
            });
        }
        if failed == "expunge" {
            first.push(Step {
                command: "UID EXPUNGE 7",
                rows: "",
                completion: Some("NO busy"),
            });
        }
        let retry = vec![
            Step::ok("SELECT Archive", SELECTED),
            Step::ok("UID SEARCH HEADER Message-ID <a@x>", "* SEARCH 81\r\n"),
            Step::ok("SELECT INBOX", SELECTED),
            Step::ok("UID STORE 7 +FLAGS (\\Deleted)", ""),
            Step::ok("UID EXPUNGE 7", "* 1 EXPUNGE\r\n"),
        ];
        let server = Server::start("IMAP4rev1 UIDPLUS", vec![first, retry]);
        let mut client = server.connect();
        assert!(
            client
                .move_messages("INBOX", "Archive", &["7"], Some("a@x"))
                .is_err()
        );
        drop(client);
        let mut client = server.connect();
        client
            .move_messages("INBOX", "Archive", &["7"], Some("a@x"))
            .unwrap();
        drop(client);
        server.finish();
    }
}

#[test]
fn a_refused_native_move_is_not_retried_as_a_copy() {
    let server = Server::start(
        "IMAP4rev1 MOVE UIDPLUS",
        vec![vec![
            Step::ok("SELECT INBOX", SELECTED),
            Step {
                command: "UID MOVE 7 Archive",
                rows: "",
                completion: Some("NO [NOPERM] denied"),
            },
        ]],
    );
    let mut client = server.connect();
    assert!(
        client
            .move_messages("INBOX", "Archive", &["7"], Some("a@x"))
            .is_err()
    );
    drop(client);
    server.finish();
}
