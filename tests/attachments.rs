//! End-to-end stand-ins for the attachments of synced mail, against a local
//! Stalwart IMAP server (A :143), spawned via `tests/stalwart2.sh` (or
//! `tests/stalwart.sh`). Ignored by default.
//!
//! io-pimdir records, in the transaction writing a message's body, a file
//! holding no body per part disposed as an attachment, in the account's
//! attachments collection, referenced from the message (pimdir STORAGE
//! §14.3). Every body neverest lands goes through that write, so:
//!   1. A sync storing bodies, primed or not, over one connection or
//!      several, leaves one stand-in per attachment (two attachments, none
//!      for an inline image, one in a nested multipart), named, typed and
//!      sized, each referenced by its message; and each message carries the
//!      attachment mark its parts give, the streamed body walked.
//!   2. A second sync writes nothing new, and the same message filed in a
//!      second folder stands for its attachments once.
//!   3. A body left owed by a run stopped after its listing records its
//!      stand-ins in the run that lands it, an arrival's under
//!      `--bodies listed` before the backlog's.
//!
//! ```sh
//! ./tests/stalwart2.sh
//! cargo test --test attachments -- --ignored --test-threads=1
//! ```

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use io_pimdir::{
    client::reader::{PimdirAccountAttachment, PimdirReader},
    placement::PimdirLinkId,
    summary::{PimdirSummary, file::PimdirFileSummary, mail},
};

const SERVER: &str = "imap://127.0.0.1:143";
const CRED: &str = "test@pimalaya.org:P!malaya-test-2026";

/// Fillers of the hydrated-later test, large enough that the run can be
/// stopped between the listing and the one batch of their bodies.
const FILLERS: usize = 30;
const FILLER_BODY: usize = 2 * 1024 * 1024;

/// The messages of a test run `id`, by name: each with its `Message-ID`
/// and its raw octets.
struct Messages {
    id: u32,
}

impl Messages {
    fn message_id(&self, name: &str) -> String {
        format!("attach-{}-{name}@pimalaya.org", self.id)
    }

    fn header(&self, name: &str, minute: usize, content_type: &str) -> String {
        format!(
            "Message-ID: <{}>\r\n\
             From: alice@pimalaya.org\r\n\
             To: bob@pimalaya.org\r\n\
             Subject: neverest attachments {name}\r\n\
             Date: Mon, 12 Oct 2026 10:{minute:02}:00 +0000\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: {content_type}\r\n",
            self.message_id(name),
        )
    }

    /// A text and two attachments: a base64 PDF and a 7bit CSV.
    fn two(&self) -> String {
        format!(
            "{}\r\n\
             --b1\r\nContent-Type: text/plain\r\n\r\nsee attached\r\n\
             --b1\r\nContent-Type: application/pdf; name=\"a.pdf\"\r\n\
             Content-Disposition: attachment; filename=\"a.pdf\"\r\n\
             Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n\
             --b1\r\nContent-Type: text/csv\r\n\
             Content-Disposition: attachment; filename=\"b.csv\"\r\n\r\na,b\r\n1,2\r\n\
             --b1--\r\n",
            self.header("two", 1, "multipart/mixed; boundary=\"b1\""),
        )
    }

    /// An inline image only: a `multipart/mixed` the header block alone
    /// marks as attaching, which it does not.
    fn inline(&self) -> String {
        format!(
            "{}\r\n\
             --b2\r\nContent-Type: text/html\r\n\r\n<img src=\"cid:i@x\">\r\n\
             --b2\r\nContent-Type: image/png\r\nContent-Disposition: inline\r\n\
             Content-ID: <i@x>\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0KGgo=\r\n\
             --b2--\r\n",
            self.header("inline", 2, "multipart/mixed; boundary=b2"),
        )
    }

    /// An attachment two levels down a signed message: the header block
    /// alone marks it as attaching nothing, which it does.
    fn nested(&self) -> String {
        format!(
            "{}\r\n\
             --outer\r\nContent-Type: multipart/mixed; boundary=inner\r\n\r\n\
             --inner\r\nContent-Type: multipart/alternative; boundary=alt\r\n\r\n\
             --alt\r\nContent-Type: text/plain\r\n\r\nhi\r\n\
             --alt\r\nContent-Type: text/html\r\n\r\n<p>hi</p>\r\n--alt--\r\n\
             --inner\r\nContent-Type: text/plain; name=notes.txt\r\n\
             Content-Disposition: attachment; filename=notes.txt\r\n\r\nsome notes\r\n\
             --inner--\r\n\
             --outer\r\nContent-Type: application/pgp-signature\r\n\r\nsig\r\n--outer--\r\n",
            self.header(
                "nested",
                3,
                "multipart/signed; boundary=outer; protocol=\"application/pgp-signature\"",
            ),
        )
    }

    fn plain(&self) -> String {
        format!(
            "{}\r\nno attachment\r\n",
            self.header("plain", 4, "text/plain")
        )
    }

    /// One attachment, arriving after the others.
    fn late(&self) -> String {
        format!(
            "{}\r\n\
             --b3\r\nContent-Type: text/plain\r\n\r\nlate\r\n\
             --b3\r\nContent-Type: application/json\r\n\
             Content-Disposition: attachment; filename=late.json\r\n\r\n{{}}\r\n\
             --b3--\r\n",
            self.header("late", 5, "multipart/mixed; boundary=b3"),
        )
    }

    /// A large message attaching nothing.
    fn filler(&self, n: usize) -> String {
        let line = "x".repeat(76);
        let body: String = (0..FILLER_BODY / 78)
            .map(|_| format!("{line}\r\n"))
            .collect();
        format!(
            "{}\r\n{body}",
            self.header(&format!("filler{n}"), 10 + n, "text/plain")
        )
    }

    /// The four messages every test seeds, by name.
    fn seeded(&self) -> Vec<(&'static str, String)> {
        vec![
            ("two", self.two()),
            ("inline", self.inline()),
            ("nested", self.nested()),
            ("plain", self.plain()),
        ]
    }
}

/// The stand-in `entry` names: name, media type, size and part.
fn described(summary: &Option<PimdirFileSummary>) -> (String, Option<String>, Option<u64>, String) {
    let summary = summary.as_ref().expect("a stand-in has its file summary");
    (
        summary.name.clone(),
        summary.media_type.clone(),
        summary.size,
        summary.part.clone().expect("a stand-in names its part"),
    )
}

/// The stand-ins of `account`, by key.
fn stand_ins(store: &Path, account: &str) -> BTreeMap<String, PimdirAccountAttachment> {
    let reader = PimdirReader::open(store).expect("open the store");
    reader
        .list_attachments_by_account(Some(account), None, 1_000)
        .expect("list the stand-ins")
        .into_iter()
        .map(|entry| (entry.link_id.0.clone(), entry))
        .collect()
}

/// The attachment mark of `name` in `folder`.
fn mark(store: &Path, folder: &str, messages: &Messages, name: &str) -> Option<bool> {
    let reader = PimdirReader::open(store).expect("open the store");
    let entry = reader
        .get_mail_row(
            format!("imap/{folder}"),
            &PimdirLinkId(messages.message_id(name)),
        )
        .expect("read the mail row")
        .unwrap_or_else(|| panic!("{name} is held in {folder}"));
    match entry.item.summary {
        Some(PimdirSummary::Mail(summary)) => summary.attachment,
        other => panic!("{name}: expected a mail summary, got {other:?}"),
    }
}

/// Asserts the stand-ins of the four seeded messages (and of `late` when
/// `late` is set): one per attachment, as the walk of each body describes
/// it, referenced by its message, in the account's attachments collection.
fn assert_stand_ins(store: &Path, account: &str, folder: &str, messages: &Messages, late: bool) {
    let mut expected: Vec<(&str, String, &str, &str, &str)> = vec![
        ("two", messages.two(), "2", "a.pdf", "application/pdf"),
        ("two", messages.two(), "3", "b.csv", "text/csv"),
        (
            "nested",
            messages.nested(),
            "1.2",
            "notes.txt",
            "text/plain",
        ),
    ];
    if late {
        expected.push((
            "late",
            messages.late(),
            "2",
            "late.json",
            "application/json",
        ));
    }

    let found = stand_ins(store, account);
    assert_eq!(
        found.len(),
        expected.len(),
        "{account}: one stand-in per attachment: {:?}",
        found.keys().collect::<Vec<_>>(),
    );
    let collection = format!("{account}/\u{1}attachments");
    for (name, body, part, file, media_type) in expected {
        let message = messages.message_id(name);
        let key = format!("part:{message}#{part}");
        let entry = found
            .get(&key)
            .unwrap_or_else(|| panic!("{account}: no stand-in {key}"));
        assert_eq!(entry.collection, collection, "{key}");
        assert_eq!(
            entry.message.0, message,
            "{key} is referenced by its message"
        );
        assert_eq!(entry.message_collection, format!("imap/{folder}"), "{key}");

        let (got_name, got_type, got_size, got_part) = described(&entry.summary);
        assert_eq!(got_name, file, "{key}");
        assert_eq!(got_type.as_deref(), Some(media_type), "{key}");
        assert_eq!(got_part, part, "{key}");
        let walked = mail::attachments::attachments(body.as_bytes())
            .into_iter()
            .find(|summary| summary.part.as_deref() == Some(part))
            .expect("the walk finds the part");
        assert_eq!(got_size, walked.size, "{key}: the decoded size");
        assert!(got_size.is_some_and(|size| size > 0), "{key}: sized");
    }
    assert_eq!(
        described(&found[&format!("part:{}#2", messages.message_id("two"))].summary).2,
        Some(9),
        "the PDF decodes to its nine octets",
    );

    let reader = PimdirReader::open(store).unwrap();
    let two = reader
        .list_attachments(Some(account), &PimdirLinkId(messages.message_id("two")))
        .unwrap();
    assert_eq!(two.len(), 2, "the message reads its two attachments");
    assert!(two.iter().all(|attachment| attachment.object.is_none()));
    let pdfs = reader
        .search_files(&[collection.as_str()], "%.pdf", None, 10)
        .unwrap();
    assert_eq!(pdfs.len(), 1, "a search of the files finds the PDF");
    let none = reader
        .list_attachments(Some(account), &PimdirLinkId(messages.message_id("inline")))
        .unwrap();
    assert!(none.is_empty(), "an inline image stands for no file");

    for (name, attaches) in [
        ("two", true),
        ("inline", false),
        ("nested", true),
        ("plain", false),
    ] {
        assert_eq!(
            mark(store, folder, messages, name),
            Some(attaches),
            "{account}: the mark of {name} is its parts'",
        );
    }
}

/// A sync storing the bodies, primed, by pages over four connections, or
/// after the listing over one, stands for each attachment once; a second
/// sync writes nothing new, and the message filed again in another folder
/// adds no stand-in.
#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :143 and --ignored"]
fn a_synced_body_stands_for_its_attachments() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let messages = Messages {
        id: std::process::id(),
    };
    let folder = format!("Attach{}", messages.id);
    let copies = format!("Attach{}Copies", messages.id);
    imap(&format!("CREATE {folder}"));
    imap(&format!("CREATE {copies}"));
    append_all(
        &folder,
        messages.seeded().into_iter().map(|(_, message)| message),
    );
    let folders = [folder.clone(), copies.clone()];

    for (account, args) in [
        ("primed", &["--download-order", "newest"][..]),
        ("paged", &["--prime", "0", "-j", "4"][..]),
        ("after", &["--prime", "0", "-j", "1"][..]),
    ] {
        let config = write_config(root, account, &folders);
        let store = root.join(account);

        let mut sync = vec!["--json", "sync", "-a", account];
        sync.extend_from_slice(args);
        neverest(&sync, &config);
        assert_stand_ins(&store, account, &folder, &messages, false);
        let before = stand_ins(&store, account);

        let report = report(&neverest(&["--json", "sync", "-a", account], &config));
        assert_eq!(fetches(&report), 0, "{account}: nothing fetched again");
        assert_eq!(stand_ins(&store, account), before, "{account}: nothing new");
    }

    // NOTE: the same body under a second folder is held already in the
    // account, so it is linked, not fetched, and walks nothing.
    imap_in(&folder, &format!("UID COPY 1:* {copies}"));
    for account in ["primed", "paged", "after"] {
        let config = root.join(format!("{account}.toml"));
        let store = root.join(account);
        let before = stand_ins(&store, account);
        neverest(&["sync", "-a", account], &config);
        assert_eq!(
            stand_ins(&store, account),
            before,
            "{account}: a body filed again stands for nothing new",
        );
        // NOTE: the copy's row keeps the mark its listing read: io-pimdir
        // links a body the account holds without fetching it, the summary
        // left at `Meta` (upgrade.rs, `LookingUp`).
        assert_eq!(
            held(&store, &copies),
            (4, 4),
            "{account}: the copies are held"
        );
        assert_eq!(mark(&store, &copies, &messages, "two"), Some(true));
        let reader = PimdirReader::open(&store).unwrap();
        let two = reader
            .list_attachments(Some(account), &PimdirLinkId(messages.message_id("two")))
            .unwrap();
        assert_eq!(two.len(), 2, "{account}: referenced once");
    }

    imap(&format!("DELETE {folder}"));
    imap(&format!("DELETE {copies}"));
}

/// Bodies a run left owed, stopped once its listing landed, stand for
/// their attachments in the run landing them: an arrival's first under
/// `--bodies listed`, the backlog's in the full run after.
#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :143 and --ignored"]
fn a_body_landing_later_stands_for_its_attachments_then() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let messages = Messages {
        id: std::process::id(),
    };
    let folder = format!("AttachLater{}", messages.id);
    imap(&format!("CREATE {folder}"));
    // NOTE: a few a command, under the server's request size.
    let fillers: Vec<String> = (0..FILLERS).map(|n| messages.filler(n)).collect();
    for chunk in fillers.chunks(5) {
        append_all(&folder, chunk.iter().cloned());
    }
    append_all(
        &folder,
        messages.seeded().into_iter().map(|(_, message)| message),
    );
    let total = FILLERS + 4;
    let config = write_config(root, "later", std::slice::from_ref(&folder));
    let store = root.join("later");

    // NOTE: over one connection the listing lands first, then one batch
    // of every body, the largest first: stopped in between, every body is
    // owed.
    let mut first = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(["sync", "-a", "later", "-j", "1", "--prime", "0"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn neverest");
    let deadline = Instant::now() + Duration::from_secs(120);
    while held(&store, &folder).0 < total {
        assert!(
            Instant::now() < deadline,
            "the listing did not land in time"
        );
        assert!(first.try_wait().unwrap().is_none(), "the run ended early");
        thread::sleep(Duration::from_millis(1));
    }
    first.kill().unwrap();
    first.wait().unwrap();
    assert_eq!(
        held(&store, &folder).1,
        0,
        "no body landed before the run was stopped (raise FILLER_BODY if it outran the kill)"
    );
    assert!(
        stand_ins(&store, "later").is_empty(),
        "no body, no stand-in"
    );

    append_all(&folder, std::iter::once(messages.late()));
    neverest(
        &["sync", "-a", "later", "-m", &folder, "--bodies", "listed"],
        &config,
    );
    let found = stand_ins(&store, "later");
    assert_eq!(
        found.keys().collect::<Vec<_>>(),
        [&format!("part:{}#2", messages.message_id("late"))],
        "the arrival's body landed, the backlog's still owed",
    );
    assert_eq!(held(&store, &folder).1, 1);

    neverest(&["sync", "-a", "later"], &config);
    assert_eq!(held(&store, &folder), (total + 1, total + 1));
    assert_stand_ins(&store, "later", &folder, &messages, true);

    imap(&format!("DELETE {folder}"));
}

/// Writes the configuration of `account`, syncing `folders` into a store
/// of its own under `root`, and initialises it.
fn write_config(root: &Path, account: &str, folders: &[String]) -> PathBuf {
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

/// How many messages `folder` holds, and how many with their body.
fn held(store: &Path, folder: &str) -> (usize, usize) {
    let Ok(reader) = PimdirReader::open(store) else {
        return (0, 0);
    };
    let items = reader
        .list_items(format!("imap/{folder}"), None, 10_000)
        .unwrap_or_default();
    let bodies = items.iter().filter(|item| item.object.is_some()).count();
    (items.len(), bodies)
}

fn report(out: &str) -> serde_json::Value {
    serde_json::from_str(out).expect("a JSON report")
}

/// The bodies a run's report says it fetched.
fn fetches(report: &serde_json::Value) -> usize {
    report["item"]["patch"]
        .as_array()
        .map(|patch| {
            patch
                .iter()
                .filter(|entry| entry["hunk"]["kind"] == "fetch")
                .count()
        })
        .unwrap_or(0)
}

/// Appends `messages` to `folder` in one IMAP command (MULTIAPPEND, RFC
/// 3502, non-synchronizing literals, RFC 7888), so the UIDs follow them.
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
    write!(writer, "b APPEND {folder}").unwrap();
    for message in messages {
        write!(writer, " {{{}+}}\r\n{message}", message.len()).unwrap();
    }
    write!(writer, "\r\n").unwrap();
    expect(&mut reader, "b");
    write!(writer, "z LOGOUT\r\n").unwrap();
}

/// Runs one IMAP command on the server, failing the test if it fails.
fn imap(command: &str) {
    let output = Command::new("curl")
        .args(["-fsS", "--user", CRED, SERVER, "-X", command])
        .output()
        .expect("spawn curl");
    assert!(output.status.success(), "IMAP `{command}` failed");
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
