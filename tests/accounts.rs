//! Every account shares one store, and `-a` filters the accounts a command
//! runs over. No server: a CalDAV endpoint is a local stub answering the
//! discovery `init` makes, or a closed port.
//!
//! ```sh
//! cargo test --test accounts
//! ```

#![cfg(feature = "dav")]

use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Output},
    thread,
};

use io_pimdir::client::{PimdirStore, reader::PimdirReader};
use serde_json::Value;

/// A port nothing listens on, so a run that connects fails.
fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// A CalDAV server answering every request with the principal and the
/// calendar home set, enough for `neverest init` to open the endpoint;
/// answers its port.
fn home_set_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            thread::spawn(move || {
                let body = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                    <d:multistatus xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\">\
                    <d:response><d:href>/</d:href><d:propstat><d:prop>\
                    <d:current-user-principal><d:href>/principal/</d:href></d:current-user-principal>\
                    <c:calendar-home-set><d:href>/calendars/</d:href></c:calendar-home-set>\
                    </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>\
                    </d:multistatus>";
                let mut request = [0; 8192];
                while let Ok(n) = stream.read(&mut request) {
                    if n == 0 {
                        break;
                    }
                    let answer = format!(
                        "HTTP/1.1 207 Multi-Status\r\n\
                         Content-Type: application/xml; charset=utf-8\r\n\
                         Content-Length: {}\r\n\r\n{body}",
                        body.len()
                    );
                    if stream.write_all(answer.as_bytes()).is_err() {
                        break;
                    }
                }
            });
        }
    });
    port
}

struct Fixture {
    _tmp: tempfile::TempDir,
    config: PathBuf,
    state: PathBuf,
}

impl Fixture {
    /// A configuration declaring `accounts`, each with one CalDAV source
    /// on `port`, `work` still claiming the `default` an earlier neverest
    /// read.
    fn new(accounts: &[&str], port: u16) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let config = tmp.path().join("config.toml");
        fs::create_dir_all(&state).unwrap();

        let mut document = String::new();
        for account in accounts {
            document.push_str(&format!("[accounts.{account}]\n"));
            if *account == "work" {
                document.push_str("default = true\n");
            }
            document.push_str(&format!(
                "caldav.server = \"http://127.0.0.1:{port}/\"\n\
                 caldav.auth.basic.username = \"user\"\n\
                 caldav.auth.basic.password.raw = \"pass\"\n\n",
            ));
        }
        fs::write(&config, document).unwrap();

        Self {
            _tmp: tmp,
            config,
            state,
        }
    }

    /// The store every account naming no root shares.
    fn store(&self) -> PathBuf {
        self.state.join("neverest")
    }

    /// The store holding `initialized` as `init` leaves them, without
    /// opening any endpoint: the database and the sidecar naming them.
    fn initialized(&self, initialized: &[&str]) {
        drop(PimdirStore::open(self.store()).unwrap());
        let entries: Vec<String> = initialized
            .iter()
            .map(|account| format!("\"{account}\":{{}}"))
            .collect();
        let sidecar = format!(r#"{{"layout":2,"accounts":{{{}}}}}"#, entries.join(","));
        fs::write(self.store().join("neverest.json"), sidecar).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_neverest"))
            .args(["-c", &self.config.to_string_lossy()])
            .args(args)
            .env("XDG_STATE_HOME", &self.state)
            .output()
            .expect("spawn neverest")
    }

    /// Every JSON line `args` printed, the result or the failure last,
    /// once it exited `code`.
    fn lines(&self, args: &[&str], code: i32) -> (Vec<Value>, String) {
        let output = self.run(args);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert_eq!(
            output.status.code(),
            Some(code),
            "`neverest {}`:\n{stdout}\n{stderr}",
            args.join(" "),
        );
        let lines = stdout
            .lines()
            .map(|line| serde_json::from_str(line).expect("a JSON line"))
            .collect();
        (lines, stderr)
    }

    /// The last JSON line `args` printed once it exited `code`.
    fn json(&self, args: &[&str], code: i32) -> Value {
        self.lines(args, code).0.pop().expect("a JSON result")
    }
}

/// Every account naming no root is initialized into one store, the
/// neverest directory itself, and a second `init` leaves them as they
/// are.
#[test]
fn every_account_is_initialized_into_one_store() {
    let fixture = Fixture::new(&["work", "home"], home_set_server());

    let init = fixture.json(&["--json", "init"], 0);

    let accounts = init["accounts"].as_array().expect("accounts");
    let names: Vec<&str> = accounts
        .iter()
        .filter_map(|a| a["account"].as_str())
        .collect();
    assert_eq!(names, ["home", "work"], "{init}");
    for account in accounts {
        assert_eq!(account["alreadyInitialized"], false, "{init}");
        assert_eq!(account["error"], Value::Null, "{init}");
        assert_eq!(
            account["store"],
            fixture.store().to_str().unwrap(),
            "{init}"
        );
    }
    assert!(fixture.store().join("pimdir.db").is_file());
    assert!(!fixture.store().join("work").exists());
    let sidecar: Value =
        serde_json::from_str(&fs::read_to_string(fixture.store().join("neverest.json")).unwrap())
            .unwrap();
    assert!(sidecar["accounts"]["home"].is_object(), "{sidecar}");
    assert!(sidecar["accounts"]["work"].is_object(), "{sidecar}");

    let again = fixture.json(&["--json", "init", "-a", "work"], 0);
    assert_eq!(again["accounts"][0]["alreadyInitialized"], true, "{again}");
}

/// A run covers every account, one after the other: one that cannot run
/// is reported with its error, the next one still runs, and the run exits
/// 3. Its events and its report name an endpoint as the configuration
/// does, and `default` is read and ignored.
#[test]
fn a_run_covers_every_account_and_reports_each_apart() {
    let fixture = Fixture::new(&["work", "home"], closed_port());
    fixture.initialized(&["work"]);

    let (lines, stderr) = fixture.lines(&["--json", "--log-level", "warn", "sync"], 3);
    let result = lines.last().unwrap();

    let home = &result["accounts"][0];
    assert_eq!(home["account"], "home", "{result}");
    assert_eq!(home["report"], Value::Null, "{result}");
    let error = home["error"].as_str().expect("an error");
    assert!(error.contains("neverest init -a home"), "{result}");

    let work = &result["accounts"][1];
    assert_eq!(work["account"], "work", "{result}");
    assert_eq!(work["error"], Value::Null, "{result}");
    assert_eq!(
        work["report"]["unreached"][0]["endpoint"], "caldav",
        "{result}"
    );

    let started: Vec<&Value> = lines
        .iter()
        .filter(|line| line["event"] == "account-started")
        .collect();
    assert_eq!(started.len(), 1, "{lines:?}");
    assert_eq!(started[0]["account"], "work");
    assert_eq!(started[0]["position"], 2);
    assert_eq!(started[0]["total"], 2);

    assert!(stderr.contains("`default` key is ignored"), "{stderr}");
}

/// The stores an earlier neverest kept per account are refused before
/// anything is created, naming each and the order that keeps their queued
/// writes, never a delete.
#[test]
fn stores_kept_per_account_are_refused_before_anything_is_created() {
    let fixture = Fixture::new(&["work", "home"], closed_port());
    for account in ["work", "home"] {
        let former = fixture.store().join(account);
        fs::create_dir_all(&former).unwrap();
        fs::write(former.join("pimdir.db"), b"").unwrap();
    }

    for command in [
        &["--json", "sync"][..],
        &["--json", "init"],
        &["--json", "drain"],
    ] {
        let failure = fixture.json(command, 1);

        assert_eq!(failure["code"], "store-layout", "{failure}");
        let error = failure["error"].as_str().unwrap_or_default();
        for account in ["work", "home"] {
            let former = fixture.store().join(account);
            assert!(error.contains(former.to_str().unwrap()), "{failure}");
        }
        assert!(error.contains("until nothing is queued"), "{failure}");
        assert!(!error.to_lowercase().contains("delete"), "{failure}");
    }
    assert!(!fixture.store().join("pimdir.db").exists());
    assert!(!fixture.store().join("sync.lock").exists());
}

/// A name the configuration does not declare fails before anything runs,
/// naming the accounts it does.
#[test]
fn an_unknown_account_fails_naming_the_configured_ones() {
    let fixture = Fixture::new(&["work", "home"], closed_port());

    let failure = fixture.json(&["--json", "sync", "-a", "perso"], 1);

    let error = failure["error"].as_str().unwrap_or_default();
    assert!(error.contains("perso"), "{failure}");
    assert!(error.contains("home, work"), "{failure}");
    assert!(!fixture.store().exists());
}

/// A reset starts one account over, so a filter leaving several is
/// refused, naming them, and nothing is reset.
#[test]
fn a_reset_needs_exactly_one_account() {
    let fixture = Fixture::new(&["work", "home"], closed_port());
    fixture.initialized(&["work", "home"]);

    let failure = fixture.json(&["--json", "sync", "--reset"], 1);

    let error = failure["error"].as_str().unwrap_or_default();
    assert!(error.contains("A reset acts on one account"), "{failure}");
    assert!(error.contains("home, work"), "{failure}");
}

/// A check names an account the store holds that no configured account
/// names, as a renamed key leaves it, and deletes nothing.
#[test]
fn a_check_names_an_account_no_configuration_names() {
    let fixture = Fixture::new(&["work"], closed_port());
    fixture.initialized(&["work"]);
    PimdirStore::open(fixture.store())
        .unwrap()
        .for_account("old")
        .ensure_collection("old/caldav/default", "text/calendar")
        .unwrap();

    let check = fixture.json(&["--json", "check"], 2);

    assert_eq!(check["orphans"][0]["account"], "old", "{check}");
    assert_eq!(
        check["orphans"][0]["store"],
        fixture.store().to_str().unwrap(),
        "{check}"
    );
    let reader = PimdirReader::open(fixture.store()).unwrap();
    assert_eq!(
        reader
            .list_collections_by_account(Some("old"))
            .unwrap()
            .len(),
        1
    );
}

/// Turning `one-way` on is refused for that account alone, naming
/// `--accept-mode`, which lets the run through and records the answer.
#[test]
fn turning_one_way_on_is_refused_until_accepted() {
    let fixture = Fixture::new(&["work"], closed_port());
    let port = closed_port();
    fs::write(
        &fixture.config,
        format!(
            "[accounts.work]\n\
             one-way = true\n\
             caldav.server = \"http://127.0.0.1:{port}/\"\n\
             caldav.auth.basic.username = \"user\"\n\
             caldav.auth.basic.password.raw = \"pass\"\n\
             targets.backup.caldav.server = \"http://127.0.0.1:{port}/\"\n\
             targets.backup.caldav.auth.basic.username = \"user\"\n\
             targets.backup.caldav.auth.basic.password.raw = \"pass\"\n",
        ),
    )
    .unwrap();
    drop(PimdirStore::open(fixture.store()).unwrap());
    let sidecar = r#"{"layout":2,"accounts":{"work":{"mode":{"sources":1,"targets":1,"one_way":false,"retain":false}}}}"#;
    fs::write(fixture.store().join("neverest.json"), sidecar).unwrap();

    let refused = fixture.json(&["--json", "sync"], 3);
    let error = refused["accounts"][0]["error"].as_str().unwrap_or_default();
    assert!(error.contains("--accept-mode"), "{refused}");

    let accepted = fixture.json(&["--json", "sync", "--accept-mode"], 3);
    assert!(accepted["accounts"][0]["report"].is_object(), "{accepted}");
    let sidecar: Value =
        serde_json::from_str(&fs::read_to_string(fixture.store().join("neverest.json")).unwrap())
            .unwrap();
    assert_eq!(
        sidecar["accounts"]["work"]["mode_accepted"], true,
        "{sidecar}"
    );

    let again = fixture.json(&["--json", "sync"], 3);
    assert!(again["accounts"][0]["report"].is_object(), "{again}");
}
