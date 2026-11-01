//! # Fake IMAP server
//!
//! A scripted IMAP4rev1 server on a local socket, for the connector tests:
//! one mailbox whose `UIDVALIDITY`, modseq and messages a test sets, and
//! which answers the commands the [backend](crate::imap::backend) sends. It
//! greets `PREAUTH` with its capabilities, so a client opens with no
//! credential, and records every command line it reads.

use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
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

/// A fake server running in the background, for as long as the test.
pub struct FakeImap {
    /// The `imap://` URL it listens on.
    pub url: Url,
    /// Every command line it read, tag stripped, in order.
    pub commands: Arc<Mutex<Vec<String>>>,
    /// The mailbox it serves, which a `STORE` changes.
    served: Arc<Mutex<FakeMailbox>>,
}

impl FakeImap {
    /// Starts a server for `mailbox`, advertising QRESYNC when `qresync`.
    pub fn start(mailbox: FakeMailbox, qresync: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let served = Arc::new(Mutex::new(mailbox));
        let commands = Arc::new(Mutex::new(Vec::new()));

        let logged = Arc::clone(&commands);
        let shared = Arc::clone(&served);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let mailbox = Arc::clone(&shared);
                let commands = Arc::clone(&logged);
                thread::spawn(move || serve(stream, qresync, &mailbox, &commands));
            }
        });

        Self {
            url: Url::parse(&format!("imap://127.0.0.1:{port}")).unwrap(),
            commands,
            served,
        }
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

/// Serves one connection until the client leaves.
fn serve(
    stream: TcpStream,
    qresync: bool,
    mailbox: &Mutex<FakeMailbox>,
    commands: &Mutex<Vec<String>>,
) {
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);

    let extensions = if qresync {
        " ENABLE CONDSTORE QRESYNC"
    } else {
        ""
    };
    let greeting = format!("* PREAUTH [CAPABILITY IMAP4rev1 UIDPLUS{extensions}] fake\r\n");
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

        let reply = answer(command, &mut mailbox.lock().unwrap());
        let done = format!("{tag} OK done\r\n");
        if writer.write_all(&reply).is_err() || writer.write_all(done.as_bytes()).is_err() {
            return;
        }
        if command.eq_ignore_ascii_case("LOGOUT") {
            return;
        }
    }
}

/// The untagged lines answering `command`.
fn answer(command: &str, mailbox: &mut FakeMailbox) -> Vec<u8> {
    let upper = command.to_ascii_uppercase();
    let mut out = Vec::new();

    if upper.starts_with("ENABLE") {
        out.extend_from_slice(b"* ENABLED CONDSTORE QRESYNC\r\n");
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
        let uids: Vec<String> = mailbox
            .messages
            .iter()
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
            let mut row = format!("* {} FETCH (UID {} FLAGS ({flags})", index + 1, message.uid);
            if let Some(section) = &section {
                let header = format!(
                    "Message-ID: <{}>\r\nDate: Thu, 29 Oct 2026 10:00:00 +0000\r\n\
                     Subject: fake\r\n\r\n",
                    message.message_id,
                );
                row.push_str(&format!(
                    " RFC822.SIZE 100 BODY[{section}] {{{}}}\r\n{header}",
                    header.len(),
                ));
            }
            row.push_str(")\r\n");
            out.extend_from_slice(row.as_bytes());
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
