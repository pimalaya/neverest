//! # Fake IMAP server
//!
//! A scripted IMAP4rev1 server on a local socket for the connector tests:
//! one mailbox a test sets, answering what the [backend](crate::imap::backend)
//! sends. It greets `PREAUTH`, so a client opens with no credential, and
//! records every command line it reads.

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

use url::Url;

/// One message of the fake mailbox.
#[derive(Clone, Debug)]
pub struct FakeMessage {
    /// Its UID under the mailbox's current `UIDVALIDITY`.
    pub uid: u32,
    /// Its flags, as the server spells them (`\Seen`).
    pub flags: Vec<String>,
    /// Its `Message-ID`, without the angle brackets.
    pub message_id: String,
}

/// The one mailbox the fake server holds, `INBOX`.
#[derive(Clone, Debug)]
pub struct FakeMailbox {
    /// The `UIDVALIDITY` a `SELECT` answers.
    pub uid_validity: u32,
    /// The `HIGHESTMODSEQ` a `SELECT` answers.
    pub highest_mod_seq: u64,
    /// Its messages, by ascending UID.
    pub messages: Vec<FakeMessage>,
    /// Whether a `SELECT` states `EXISTS`, which a server may leave out.
    pub states_exists: bool,
}

/// How the fake breaks its next `UID FETCH` of whole bodies.
#[derive(Clone, Copy, Debug)]
pub enum FakeFault {
    /// Refuses the command with a tagged `NO`.
    Refuse,
    /// Answers the first body twice, the rest of the answer following.
    RepeatBody,
    /// Answers the first body, then closes the connection.
    HangUp,
}

/// A fake server running in the background, for as long as the test.
pub struct FakeImap {
    /// The `imap://` URL it listens on.
    pub url: Url,
    /// Every command line it read, tag stripped, in order.
    pub commands: Arc<Mutex<Vec<String>>>,
    /// How many connections it accepted.
    pub connections: Arc<AtomicUsize>,
    /// The mailbox it serves, which a `STORE` changes.
    served: Arc<Mutex<FakeMailbox>>,
    /// The header block a message states in place of the fake's own, by UID.
    headers: Arc<Mutex<BTreeMap<u32, Vec<u8>>>>,
    /// The fault its next `UID FETCH` of whole bodies meets.
    fault: Arc<Mutex<Option<FakeFault>>>,
    /// The completion its next `APPEND` or `UID STORE` is refused with.
    refusal: Arc<Mutex<Option<&'static str>>>,
    /// The completion its next `APPEND` is refused with at its command line,
    /// before the literal is sent.
    early: Arc<Mutex<Option<&'static str>>>,
    /// The completion every search for the messages with no `Date` (`NOT
    /// HEADER Date ""`) is refused with, as iCloud refuses it.
    undated: Arc<Mutex<Option<&'static str>>>,
    /// Whether an `ENABLE` enables what it asks, else answered a bare `OK`
    /// with no `ENABLED`, as iCloud answers it.
    enables: Arc<AtomicBool>,
    /// Whether an `APPEND` adds a message and names it by `APPENDUID`, else
    /// its literal is dropped and nothing named.
    keeps: Arc<AtomicBool>,
    /// Whether the greeting announces UIDPLUS (RFC 4315), else a kept
    /// `APPEND` names no `APPENDUID`.
    uidplus: Arc<AtomicBool>,
    /// Whether the greeting announces MOVE (RFC 6851), else a `UID MOVE` is
    /// answered `BAD`.
    moves: Arc<AtomicBool>,
    /// The `Message-ID` another client delivers right after the next kept
    /// `APPEND`, once.
    delivers: Arc<Mutex<Option<String>>>,
}

impl FakeImap {
    /// Starts a server for `mailbox`, advertising QRESYNC when `qresync`.
    pub fn start(mailbox: FakeMailbox, qresync: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let served = Arc::new(Mutex::new(mailbox));
        let commands = Arc::new(Mutex::new(Vec::new()));
        let headers = Arc::new(Mutex::new(BTreeMap::new()));
        let connections = Arc::new(AtomicUsize::new(0));
        let fault = Arc::new(Mutex::new(None));
        let refusal = Arc::new(Mutex::new(None));
        let early = Arc::new(Mutex::new(None));
        let undated = Arc::new(Mutex::new(None));
        let enables = Arc::new(AtomicBool::new(true));
        let keeps = Arc::new(AtomicBool::new(false));
        let uidplus = Arc::new(AtomicBool::new(true));
        let moves = Arc::new(AtomicBool::new(true));
        let delivers = Arc::new(Mutex::new(None));

        let logged = Arc::clone(&commands);
        let shared = Arc::clone(&served);
        let stated = Arc::clone(&headers);
        let accepted = Arc::clone(&connections);
        let broken = Arc::clone(&fault);
        let refused = Arc::clone(&refusal);
        let refused_early = Arc::clone(&early);
        let blind = Arc::clone(&undated);
        let enabling = Arc::clone(&enables);
        let keeping = Arc::clone(&keeps);
        let announcing = Arc::clone(&uidplus);
        let moving = Arc::clone(&moves);
        let delivering = Arc::clone(&delivers);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                accepted.fetch_add(1, Ordering::SeqCst);
                let mailbox = Arc::clone(&shared);
                let commands = Arc::clone(&logged);
                let headers = Arc::clone(&stated);
                let fault = Arc::clone(&broken);
                let refusal = Arc::clone(&refused);
                let early = Arc::clone(&refused_early);
                let undated = Arc::clone(&blind);
                let enables = Arc::clone(&enabling);
                let keeps = Arc::clone(&keeping);
                let uidplus = Arc::clone(&announcing);
                let moves = Arc::clone(&moving);
                let delivers = Arc::clone(&delivering);
                thread::spawn(move || {
                    let refusals = Refusals {
                        write: &refusal,
                        early: &early,
                        undated: &undated,
                        enables: &enables,
                        keeps: &keeps,
                        uidplus: &uidplus,
                        moves: &moves,
                        delivers: &delivers,
                    };
                    serve(
                        stream, qresync, &mailbox, &headers, &commands, &fault, refusals,
                    )
                });
            }
        });

        Self {
            url: Url::parse(&format!("imap://127.0.0.1:{port}")).unwrap(),
            commands,
            connections,
            served,
            headers,
            fault,
            refusal,
            early,
            undated,
            enables,
            keeps,
            uidplus,
            moves,
            delivers,
        }
    }

    /// Has every `ENABLE` answered a bare `OK`, enabling nothing, as iCloud
    /// answers `ENABLE CONDSTORE QRESYNC` while advertising both.
    pub fn enable_nothing(&self) {
        self.enables.store(false, Ordering::SeqCst);
    }

    /// Has every `APPEND` add a message, named by `APPENDUID`.
    pub fn keep_appends(&self) {
        self.keeps.store(true, Ordering::SeqCst);
    }

    /// Has the greeting announce no UIDPLUS, for the connections opened
    /// after.
    pub fn without_uidplus(&self) {
        self.uidplus.store(false, Ordering::SeqCst);
    }

    /// Has the greeting announce no MOVE, for the connections opened after,
    /// each `UID MOVE` then answered `BAD` as OVH answers it.
    pub fn without_move(&self) {
        self.moves.store(false, Ordering::SeqCst);
    }

    /// Has another client deliver a message stating `message_id` right after
    /// the next kept `APPEND`, before any search, once.
    pub fn deliver_after_next_append(&self, message_id: &str) {
        *self.delivers.lock().unwrap() = Some(message_id.to_owned());
    }

    /// Has every search for the messages with no `Date` refused with the
    /// tagged `completion` (`NO [UNAVAILABLE] Unexpected exception`).
    pub fn refuse_undated_search(&self, completion: &'static str) {
        *self.undated.lock().unwrap() = Some(completion);
    }

    /// Has the next `UID FETCH` of whole bodies meet `fault`, once.
    pub fn break_next_body_fetch(&self, fault: FakeFault) {
        *self.fault.lock().unwrap() = Some(fault);
    }

    /// Has the next `APPEND` or `UID STORE` refused with the tagged
    /// `completion` (`NO [NOPERM] denied`), once.
    pub fn refuse_next_write(&self, completion: &'static str) {
        *self.refusal.lock().unwrap() = Some(completion);
    }

    /// Has the next `APPEND` refused with the tagged `completion` at its
    /// command line, its literal never asked for, once.
    pub fn refuse_next_append_line(&self, completion: &'static str) {
        *self.early.lock().unwrap() = Some(completion);
    }

    /// Has the message `uid` state `header`, a whole header block, where a
    /// `FETCH` asks for header fields.
    pub fn set_header(&self, uid: u32, header: &[u8]) {
        self.headers.lock().unwrap().insert(uid, header.to_vec());
    }

    /// Has the mailbox hold `messages` under `uid_validity`, as a server
    /// rebuilding it renumbers every message.
    pub fn renumber(&self, uid_validity: u32, messages: Vec<FakeMessage>) {
        let mut mailbox = self.served.lock().unwrap();
        mailbox.uid_validity = uid_validity;
        mailbox.messages = messages;
    }

    /// The flags the message `uid` holds now.
    pub fn flags(&self, uid: u32) -> Vec<String> {
        let mailbox = self.served.lock().unwrap();
        let message = mailbox.messages.iter().find(|message| message.uid == uid);
        message
            .map(|message| message.flags.clone())
            .unwrap_or_default()
    }

    /// Sets the flags of the message `uid`, as another client would.
    pub fn set_flags(&self, uid: u32, flags: &[&str]) {
        let mut mailbox = self.served.lock().unwrap();
        if let Some(message) = mailbox
            .messages
            .iter_mut()
            .find(|message| message.uid == uid)
        {
            message.flags = flags.iter().map(|flag| flag.to_string()).collect();
        }
    }

    /// The command lines read so far whose verb is `verb`, tag stripped.
    pub fn sent(&self, verb: &str) -> Vec<String> {
        self.commands
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.to_ascii_uppercase().starts_with(verb))
            .cloned()
            .collect()
    }
}

/// The commands a server refuses.
///
/// Its next write, once, every search for the messages with no `Date`, what
/// an `ENABLE` asks when it enables nothing, and the UIDPLUS and MOVE it may
/// lack.
struct Refusals<'a> {
    write: &'a Mutex<Option<&'static str>>,
    early: &'a Mutex<Option<&'static str>>,
    undated: &'a Mutex<Option<&'static str>>,
    enables: &'a AtomicBool,
    keeps: &'a AtomicBool,
    uidplus: &'a AtomicBool,
    moves: &'a AtomicBool,
    delivers: &'a Mutex<Option<String>>,
}

/// Serves one connection until the client leaves.
///
/// An `APPEND` literal is dropped with no `APPENDUID`, unless the server
/// keeps appends, its `Message-ID` naming the message. A fault cuts a reply
/// after its first row, found by its closing `)`: no fake body ends so.
fn serve(
    stream: TcpStream,
    qresync: bool,
    mailbox: &Mutex<FakeMailbox>,
    headers: &Mutex<BTreeMap<u32, Vec<u8>>>,
    commands: &Mutex<Vec<String>>,
    fault: &Mutex<Option<FakeFault>>,
    refusals: Refusals<'_>,
) {
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);

    let extensions = if qresync {
        " ENABLE CONDSTORE QRESYNC"
    } else {
        ""
    };
    let uidplus = refusals.uidplus.load(Ordering::SeqCst);
    let announced = if uidplus { " UIDPLUS" } else { "" };
    let moves = refusals.moves.load(Ordering::SeqCst);
    let moving = if moves { " MOVE" } else { "" };
    let greeting =
        format!("* PREAUTH [CAPABILITY IMAP4rev1{announced}{moving}{extensions}] fake\r\n");
    if writer.write_all(greeting.as_bytes()).is_err() {
        return;
    }

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let line = line.trim_end();
        let Some((tag, command)) = line.split_once(' ') else {
            continue;
        };
        commands.lock().unwrap().push(command.to_owned());

        let appending = command.to_ascii_uppercase().starts_with("APPEND");
        if let Some(completion) = appending
            .then(|| refusals.early.lock().unwrap().take())
            .flatten()
        {
            if writer
                .write_all(format!("{tag} {completion}\r\n").as_bytes())
                .is_err()
            {
                return;
            }
            continue;
        }

        let mut appended = String::new();
        if let Some(octets) = literal_of(command) {
            let mut literal = vec![0; octets];
            let mut rest = String::new();
            if writer.write_all(b"+ go ahead\r\n").is_err()
                || reader.read_exact(&mut literal).is_err()
                || reader.read_line(&mut rest).is_err()
            {
                return;
            }
            appended = message_id_of(&literal);
        }

        let upper = command.to_ascii_uppercase();
        let writes = upper.starts_with("APPEND") || upper.starts_with("UID STORE");
        let undated = upper.starts_with("UID SEARCH") && upper.contains("HEADER DATE \"\"");
        let enabling = upper.starts_with("ENABLE") && !refusals.enables.load(Ordering::SeqCst);
        let unmoved = upper.starts_with("UID MOVE") && !moves;
        let refused = match (writes, undated, enabling, unmoved) {
            (true, _, _, _) => refusals.write.lock().unwrap().take(),
            (_, true, _, _) => *refusals.undated.lock().unwrap(),
            (_, _, true, _) => Some("OK nothing enabled"),
            (_, _, _, true) => Some("BAD parse error: command not permitted with UID"),
            _ => None,
        };
        if let Some(completion) = refused {
            if writer
                .write_all(format!("{tag} {completion}\r\n").as_bytes())
                .is_err()
            {
                return;
            }
            continue;
        }

        let mut reply = answer(
            command,
            &mut mailbox.lock().unwrap(),
            &headers.lock().unwrap(),
        );

        let whole_bodies = section_of(command).is_some_and(|section| section.is_empty());
        let fault = match whole_bodies {
            true => fault.lock().unwrap().take(),
            false => None,
        };
        let first = reply
            .windows(3)
            .position(|window| window == b")\r\n")
            .map(|at| at + 3);
        match (fault, first) {
            (Some(FakeFault::Refuse), _) => {
                let refused = format!("{tag} NO refused\r\n");
                if writer.write_all(refused.as_bytes()).is_err() {
                    return;
                }
                continue;
            }
            (Some(FakeFault::RepeatBody), Some(first)) => {
                let row = reply[..first].to_vec();
                reply.splice(..0, row);
            }
            (Some(FakeFault::HangUp), Some(first)) => {
                let _ = writer.write_all(&reply[..first]);
                return;
            }
            _ => {}
        }

        let done = match appending && refusals.keeps.load(Ordering::SeqCst) {
            true => {
                let mut mailbox = mailbox.lock().unwrap();
                let uid = mailbox.messages.last().map_or(1, |message| message.uid + 1);
                mailbox.messages.push(FakeMessage {
                    uid,
                    flags: Vec::new(),
                    message_id: appended,
                });
                if let Some(delivered) = refusals.delivers.lock().unwrap().take() {
                    mailbox.messages.push(FakeMessage {
                        uid: uid + 1,
                        flags: Vec::new(),
                        message_id: delivered,
                    });
                }
                let validity = mailbox.uid_validity;
                match uidplus {
                    true => format!("{tag} OK [APPENDUID {validity} {uid}] done\r\n"),
                    false => format!("{tag} OK done\r\n"),
                }
            }
            false => format!("{tag} OK done\r\n"),
        };
        if writer.write_all(&reply).is_err() || writer.write_all(done.as_bytes()).is_err() {
            return;
        }
        if command.eq_ignore_ascii_case("LOGOUT") {
            return;
        }
    }
}

/// The `Message-ID` an appended message states, bare, empty when none.
fn message_id_of(message: &[u8]) -> String {
    let text = String::from_utf8_lossy(message);
    text.lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("Message-ID").then_some(value)
        })
        .map(|value| value.trim().trim_matches(['<', '>']).to_owned())
        .unwrap_or_default()
}

/// The octets of the synchronising literal `command` ends on (`{12}`).
fn literal_of(command: &str) -> Option<usize> {
    let (_, octets) = command.strip_suffix('}')?.rsplit_once('{')?;
    octets.parse().ok()
}

/// The untagged lines answering `command`.
///
/// A `LIST` names `INBOX` alone, whatever its pattern. A `UID SEARCH`
/// matches everything unless its criterion is a `Message-ID`.
fn answer(command: &str, mailbox: &mut FakeMailbox, headers: &BTreeMap<u32, Vec<u8>>) -> Vec<u8> {
    let upper = command.to_ascii_uppercase();
    let mut out = Vec::new();

    if upper.starts_with("ENABLE") {
        out.extend_from_slice(b"* ENABLED CONDSTORE QRESYNC\r\n");
    } else if upper.starts_with("LIST") {
        out.extend_from_slice(b"* LIST () \"/\" INBOX\r\n");
    } else if upper.starts_with("SELECT") || upper.starts_with("EXAMINE") {
        if mailbox.states_exists {
            let exists = mailbox.messages.len();
            out.extend_from_slice(format!("* {exists} EXISTS\r\n").as_bytes());
        }
        let next = mailbox.messages.last().map_or(1, |message| message.uid + 1);
        out.extend_from_slice(
            format!(
                "* OK [UIDVALIDITY {}] fake\r\n\
                 * OK [UIDNEXT {next}] fake\r\n\
                 * OK [HIGHESTMODSEQ {}] fake\r\n",
                mailbox.uid_validity, mailbox.highest_mod_seq,
            )
            .as_bytes(),
        );
    } else if upper.starts_with("UID SEARCH") {
        let header = upper
            .find("HEADER MESSAGE-ID ")
            .map(|at| command[at + 18..].trim_matches('"'));
        let uids: Vec<String> = mailbox
            .messages
            .iter()
            .filter(|message| {
                header.is_none_or(|id| format!("<{}>", message.message_id).contains(id))
            })
            .map(|message| message.uid.to_string())
            .collect();
        out.extend_from_slice(format!("* SEARCH {}\r\n", uids.join(" ")).as_bytes());
    } else if upper.starts_with("UID STORE") {
        store(command, mailbox);
    } else if upper.starts_with("UID FETCH") {
        let set = command.split(' ').nth(2).unwrap_or_default();
        let section = section_of(command);
        for (index, message) in mailbox.messages.iter().enumerate() {
            if !in_set(set, message.uid) {
                continue;
            }
            let flags = message.flags.join(" ");
            let row = format!("* {} FETCH (UID {} FLAGS ({flags})", index + 1, message.uid);
            out.extend_from_slice(row.as_bytes());
            if let Some(section) = &section {
                let header = headers.get(&message.uid).cloned().unwrap_or_else(|| {
                    format!(
                        "Message-ID: <{}>\r\nDate: Thu, 29 Oct 2026 10:00:00 +0000\r\n\
                         Subject: fake\r\n\r\n",
                        message.message_id,
                    )
                    .into_bytes()
                });
                let item = format!(" RFC822.SIZE 100 BODY[{section}] {{{}}}\r\n", header.len());
                out.extend_from_slice(item.as_bytes());
                out.extend_from_slice(&header);
            }
            out.extend_from_slice(b")\r\n");
        }
    }

    out
}

/// Applies a `UID STORE <set> [+-]FLAGS[.SILENT] (<flags>)` to `mailbox`,
/// answering nothing, as `.SILENT` asks.
fn store(command: &str, mailbox: &mut FakeMailbox) {
    let mut words = command.splitn(5, ' ');
    let (Some(set), Some(item)) = (words.nth(2), words.next()) else {
        return;
    };
    let named = words.next().unwrap_or_default();
    let named: Vec<String> = named
        .trim_matches(|c| c == '(' || c == ')')
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let item = item.to_ascii_uppercase();

    for message in &mut mailbox.messages {
        if !in_set(set, message.uid) {
            continue;
        }
        if item.starts_with('+') {
            for flag in &named {
                if !message.flags.contains(flag) {
                    message.flags.push(flag.clone());
                }
            }
        } else if item.starts_with('-') {
            message.flags.retain(|flag| !named.contains(flag));
        } else {
            message.flags = named.clone();
        }
    }
}

/// The section a `BODY.PEEK[...]` asks for, echoed by the answer.
fn section_of(command: &str) -> Option<String> {
    let start = command.find("BODY.PEEK[")? + "BODY.PEEK[".len();
    let len = command[start..].find(']')?;
    Some(command[start..start + len].to_owned())
}

/// Whether `uid` is in an IMAP UID set such as `1:4,7,9:*`.
fn in_set(set: &str, uid: u32) -> bool {
    let bound = |raw: &str| match raw {
        "*" => u32::MAX,
        raw => raw.parse().unwrap_or(0),
    };
    set.split(',').any(|part| match part.split_once(':') {
        Some((from, to)) => {
            let (from, to) = (bound(from), bound(to));
            (from.min(to)..=from.max(to)).contains(&uid)
        }
        None => bound(part) == uid,
    })
}
