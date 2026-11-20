//! # Servers without UIDPLUS
//!
//! The `Remove` and `Add` contracts of pimdir SYNC §4 against a real IMAP
//! server lacking UIDPLUS (RFC 4315): an append answers no `APPENDUID`, and
//! no single message can be expunged alone.
//!
//! The server is maddy, whose binary `NEVEREST_MADDY` names (the dev shell
//! sets it). Each test starts its own on loopback, its state in a temporary
//! directory, and passes untested when the variable is unset:
//!
//! ```sh
//! nix develop --command cargo test --test without_uidplus
//! ```

#![cfg(feature = "imap")]

use std::{
    env,
    ffi::OsString,
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, MutexGuard, PoisonError},
    thread,
    time::{Duration, Instant},
};

use io_pimdir::{
    client::{producer::PimdirProducer, reader::PimdirReader},
    codec::PimdirAction,
    collection::PimdirCollectionId,
    placement::{PimdirFlags, PimdirLinkId},
};
use serde_json::Value;
use tempfile::TempDir;

const MADDY_VAR: &str = "NEVEREST_MADDY";
const USER: &str = "test@localhost";
const PASS: &str = "neverest-test";
const ACCOUNT: &str = "y2";

/// Held by each test for its whole run.
///
/// A child another test spawns holds every lock this process holds until its
/// exec, so a store opened here beside a spawn reads as owned.
static SPAWNS: Mutex<()> = Mutex::new(());

fn serialized() -> MutexGuard<'static, ()> {
    SPAWNS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A maddy server on loopback, killed on drop.
struct Maddy {
    child: Child,
    port: u16,
    dir: TempDir,
}

impl Maddy {
    /// Starts maddy with one user, `None` when `NEVEREST_MADDY` is unset.
    fn start() -> Option<Self> {
        let Some(bin) = env::var_os(MADDY_VAR) else {
            eprintln!("{MADDY_VAR} unset, no maddy to test against");
            return None;
        };
        let dir = tempfile::tempdir().expect("maddy dir");
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .expect("a free port")
            .port();
        let state = dir.path().join("state");
        let runtime = dir.path().join("run");
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        let config = dir.path().join("maddy.conf");
        fs::write(
            &config,
            format!(
                "hostname localhost\n\
                 state_dir {}\n\
                 runtime_dir {}\n\
                 tls off\n\
                 auth.pass_table local_authdb {{\n\
                     table sql_table {{\n\
                         driver sqlite3\n\
                         dsn credentials.db\n\
                         table_name passwords\n\
                     }}\n\
                 }}\n\
                 storage.imapsql local_mailboxes {{\n\
                     driver sqlite3\n\
                     dsn imapsql.db\n\
                 }}\n\
                 imap tcp://127.0.0.1:{port} {{\n\
                     auth &local_authdb\n\
                     storage &local_mailboxes\n\
                     insecure_auth yes\n\
                 }}\n",
                state.display(),
                runtime.display(),
            ),
        )
        .unwrap();

        maddy_cli(
            &bin,
            &config,
            &["creds", "create", "--bcrypt-cost", "4", USER],
        );
        maddy_cli(&bin, &config, &["imap-acct", "create", USER]);

        let log = fs::File::create(dir.path().join("maddy.log")).unwrap();
        let child = Command::new(&bin)
            .arg("--config")
            .arg(&config)
            .arg("run")
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("spawn maddy");
        let maddy = Self { child, port, dir };

        let deadline = Instant::now() + Duration::from_secs(20);
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(
                Instant::now() < deadline,
                "maddy did not listen:\n{}",
                maddy.log(),
            );
            thread::sleep(Duration::from_millis(50));
        }

        Some(maddy)
    }

    fn log(&self) -> String {
        fs::read_to_string(self.dir.path().join("maddy.log")).unwrap_or_default()
    }

    /// A session of another client, logged in.
    fn session(&self) -> Session {
        Session::open(self.port)
    }
}

impl Drop for Maddy {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Runs a maddy subcommand over `config`, the password on its stdin.
fn maddy_cli(bin: &OsString, config: &Path, args: &[&str]) {
    let mut child = Command::new(bin)
        .arg("--config")
        .arg(config)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn maddy");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(PASS.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "`maddy {}` failed:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// A bare IMAP client, the other client of the server.
struct Session {
    stream: BufReader<TcpStream>,
    tag: u32,
}

impl Session {
    fn open(port: u16) -> Self {
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to maddy");
        let mut session = Self {
            stream: BufReader::new(stream),
            tag: 0,
        };
        let greeting = session.line();
        assert!(greeting.starts_with("* OK"), "{greeting}");
        session.run(&format!("LOGIN \"{USER}\" \"{PASS}\""));
        session
    }

    fn line(&mut self) -> String {
        let mut line = String::new();
        self.stream.read_line(&mut line).expect("read from maddy");
        assert!(!line.is_empty(), "maddy closed the connection");
        line.trim_end().to_string()
    }

    /// Sends `command` and answers its untagged lines, failing unless `OK`.
    fn run(&mut self, command: &str) -> Vec<String> {
        self.tag += 1;
        let tag = format!("t{}", self.tag);
        write!(self.stream.get_mut(), "{tag} {command}\r\n").unwrap();
        let mut untagged = Vec::new();
        loop {
            let line = self.line();
            let Some(status) = line.strip_prefix(&format!("{tag} ")) else {
                untagged.push(line);
                continue;
            };
            assert!(status.starts_with("OK"), "`{command}`: {line}");
            return untagged;
        }
    }

    /// The capabilities the server states once logged in.
    fn capabilities(&mut self) -> Vec<String> {
        self.run("CAPABILITY")
            .iter()
            .filter_map(|line| line.strip_prefix("* CAPABILITY "))
            .flat_map(|caps| caps.split(' ').map(str::to_string).collect::<Vec<_>>())
            .collect()
    }

    /// Appends `body` to `mailbox` as a synchronizing-free literal.
    fn append(&mut self, mailbox: &str, body: &str) {
        self.run(&format!("APPEND {mailbox} {{{}+}}\r\n{body}", body.len()));
    }

    /// The UIDs of the messages of `mailbox` stating `message_id`.
    fn uids(&mut self, mailbox: &str, message_id: &str) -> Vec<u32> {
        self.run(&format!("EXAMINE {mailbox}"));
        self.run(&format!("UID SEARCH HEADER Message-ID \"<{message_id}>\""))
            .iter()
            .filter_map(|line| line.strip_prefix("* SEARCH"))
            .flat_map(|uids| {
                uids.split_whitespace()
                    .map(|uid| uid.parse::<u32>().unwrap())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// The `FLAGS` item the server answers for `uid` of `mailbox`.
    fn flags(&mut self, mailbox: &str, uid: u32) -> String {
        self.run(&format!("EXAMINE {mailbox}"));
        let fetched = self.run(&format!("UID FETCH {uid} (FLAGS)"));
        assert_eq!(fetched.len(), 1, "{fetched:?}");
        fetched[0].clone()
    }
}

/// A neverest client of the server, with its own store.
struct Client {
    dir: TempDir,
}

impl Client {
    /// Writes the account syncing `mailboxes` and initializes its store.
    fn init(maddy: &Maddy, mailboxes: &[&str]) -> Self {
        let dir = tempfile::tempdir().expect("client dir");
        let include = mailboxes
            .iter()
            .map(|mailbox| format!("\"{mailbox}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let client = Self { dir };
        fs::write(
            client.config(),
            format!(
                "[accounts.{ACCOUNT}]\n\
                 store.root = \"{}\"\n\
                 imap.server = \"imap://127.0.0.1:{}\"\n\
                 imap.starttls = false\n\
                 imap.sasl.plain.username = \"{USER}\"\n\
                 imap.sasl.plain.password.raw = \"{PASS}\"\n\
                 imap.collection.filter.include = [{include}]\n",
                client.store().display(),
                maddy.port,
            ),
        )
        .unwrap();
        let (code, stdout) = client.run(&["init", "-a", ACCOUNT]);
        assert_eq!(code, Some(0), "{stdout}");
        client
    }

    fn config(&self) -> PathBuf {
        self.dir.path().join("config.toml")
    }

    fn store(&self) -> PathBuf {
        self.dir.path().join("store")
    }

    fn run(&self, args: &[&str]) -> (Option<i32>, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
            .arg("-c")
            .arg(self.config())
            .args(args)
            .env("XDG_STATE_HOME", self.dir.path().join("state"))
            .output()
            .expect("spawn neverest");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr);
        (output.status.code(), format!("{stdout}\n{stderr}"))
    }

    /// Syncs the account with `args`, answering its exit code and report.
    fn sync(&self, args: &[&str]) -> (Option<i32>, Value) {
        let mut sync = vec!["--json", "sync", "-a", ACCOUNT];
        sync.extend_from_slice(args);
        let (code, output) = self.run(&sync);
        let report = serde_json::Deserializer::from_str(&output)
            .into_iter::<Value>()
            .filter_map(Result::ok)
            .last()
            .unwrap_or_else(|| panic!("no JSON result:\n{output}"));
        (code, report["accounts"][0]["report"].clone())
    }

    /// The seqs and link ids of the live items of `mailbox`.
    fn items(&self, mailbox: &str) -> Vec<(i64, String)> {
        PimdirReader::open(self.store())
            .expect("open the store")
            .list_summaries(collection(mailbox), None, 1000)
            .expect("list the collection")
            .into_iter()
            .map(|item| (item.seq, item.link_id.0))
            .collect()
    }

    /// The handles the sources bind the item `link_id` of `mailbox` by.
    fn handles(&self, mailbox: &str, link_id: &str) -> Vec<String> {
        PimdirReader::open(self.store())
            .expect("open the store")
            .item_bindings(collection(mailbox), link_id)
            .expect("read the bindings")
            .into_values()
            .map(|binding| binding.handle.0)
            .collect()
    }

    fn producer(&self) -> PimdirProducer {
        PimdirProducer::open(self.store(), "neverest-tests").expect("open the producer")
    }
}

fn collection(mailbox: &str) -> String {
    format!("{ACCOUNT}/imap/{mailbox}")
}

fn message(message_id: &str, subject: &str) -> String {
    format!(
        "Message-ID: <{message_id}>\r\n\
         From: alice@localhost\r\n\
         To: bob@localhost\r\n\
         Subject: {subject}\r\n\
         Date: Wed, 07 Oct 2026 10:00:00 +0000\r\n\
         \r\n\
         {subject}\r\n"
    )
}

/// The server is one this suite is about: no `UID EXPUNGE`, no `APPENDUID`.
fn lacks_uidplus(maddy: &Maddy) {
    let capabilities = maddy.session().capabilities();
    assert!(!capabilities.is_empty());
    assert!(
        !capabilities
            .iter()
            .any(|cap| cap.eq_ignore_ascii_case("UIDPLUS") || cap.eq_ignore_ascii_case("IMAP4rev2")),
        "{capabilities:?}"
    );
}

/// A move into a mailbox already holding its `Message-ID` is held: the
/// member cannot be expunged alone, so nothing is filed a second time and
/// nothing is marked `\Deleted`, run after run, a change another client made
/// to the member since included (pimdir SYNC §4, §5).
///
/// The first run lists the source alone, so the push asks the target; the
/// next ones list the target too, its copy landing the move's create first.
#[test]
fn a_move_into_a_mailbox_holding_its_identity_is_held() {
    let _serial = serialized();
    let Some(maddy) = Maddy::start() else {
        return;
    };
    lacks_uidplus(&maddy);

    const ID: &str = "held@localhost";
    let mut other = maddy.session();
    other.run("CREATE Held");
    other.run("CREATE Filed");
    other.append("Held", &message(ID, "held"));

    let client = Client::init(&maddy, &["Held", "Filed"]);
    let (code, report) = client.sync(&[]);
    assert_eq!(code, Some(0), "{report}");
    let held = client.items("Held");
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0].1, ID);

    let mut producer = client.producer();
    let filing = PimdirAction::Move {
        seq: held[0].0,
        to: PimdirCollectionId(collection("Filed")),
    };
    producer
        .enqueue(&collection("Held"), &filing, None)
        .unwrap();
    drop(producer);
    other.append("Filed", &message(ID, "held"));

    let source = other.uids("Held", ID);
    assert_eq!(source.len(), 1);
    let narrowed: &[&str] = &["-m", "Held"];
    for (run, args) in [(1, narrowed), (2, &[]), (3, &[])] {
        if run == 3 {
            other.run("SELECT Held");
            other.run(&format!("UID STORE {} +FLAGS (\\Flagged)", source[0]));
        }

        let (code, report) = client.sync(args);

        assert_eq!(code, Some(3), "run {run}: {report}");
        let rejected = report["errors"].as_array().unwrap();
        assert_eq!(rejected.len(), 1, "run {run}: {report}");
        assert_eq!(rejected[0]["kind"], "rejected", "run {run}: {report}");
        assert_eq!(rejected[0]["action"], "delete", "run {run}: {report}");
        assert_eq!(rejected[0]["collection"], "Held", "run {run}: {report}");
        assert_eq!(
            other.uids("Held", ID),
            source,
            "run {run}: the member stays"
        );
        let flags = other.flags("Held", source[0]);
        assert!(!flags.contains("\\Deleted"), "run {run}: {flags}");
        assert_eq!(
            other.uids("Filed", ID).len(),
            1,
            "run {run}: filed no second time"
        );
        if run > 1 {
            assert_eq!(client.items("Filed").len(), 1, "run {run}");
        }
        if run == 3 {
            assert!(flags.contains("\\Flagged"), "run {run}: {flags}");
        }
    }
}

/// An append answered with no `APPENDUID` is bound to the UID a search on
/// its `Message-ID` finds: the server holds one copy, and a second run
/// changes nothing (pimdir SYNC §4).
#[test]
fn an_append_without_appenduid_is_bound_by_its_message_id() {
    let _serial = serialized();
    let Some(maddy) = Maddy::start() else {
        return;
    };
    lacks_uidplus(&maddy);

    const ID: &str = "appended@localhost";
    let mut other = maddy.session();
    other.run("CREATE Box");

    let client = Client::init(&maddy, &["Box"]);
    let (code, report) = client.sync(&[]);
    assert_eq!(code, Some(0), "{report}");

    client
        .producer()
        .enqueue_with_body(
            &collection("Box"),
            message(ID, "appended").as_bytes(),
            |object| PimdirAction::Add {
                link_id: Some(PimdirLinkId(ID.into())),
                flags: PimdirFlags::default(),
                object: Some(object),
                supersedes: None,
            },
        )
        .unwrap();

    let (code, report) = client.sync(&[]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["errors"], serde_json::json!([]), "{report}");
    let added = report["itemPatch"].as_array().unwrap();
    assert_eq!(added.len(), 1, "{report}");
    assert_eq!(added[0]["kind"], "add", "{report}");
    let uids = other.uids("Box", ID);
    assert_eq!(uids.len(), 1, "appended once");
    assert_eq!(client.handles("Box", ID), [uids[0].to_string()]);

    let (code, report) = client.sync(&[]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["itemPatch"], serde_json::json!([]), "{report}");
    assert_eq!(report["downloaded"], serde_json::json!([]), "{report}");
    assert_eq!(other.uids("Box", ID).len(), 1, "appended once");
    let items = client.items("Box");
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].1, ID);
}
