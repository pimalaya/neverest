//! # Exit contract
//!
//! The built binary's exit statuses beyond success and failure. Runs
//! without any server: the endpoint down is a closed local port.
//!
//! A command line the grammar refuses exits 2, coded `usage-invalid` under
//! `--json`; a standard command missing its argument fails like any other;
//! a check that printed its report with an endpoint down exits 3, and a
//! drain that left an account undrained exits 4.

use std::{
    fs,
    net::TcpListener,
    path::Path,
    process::{Command, Output, Stdio},
    thread,
    time::Duration,
};

use serde_json::Value;
use tempfile::TempDir;

/// Runs the binary with `args` in `home`, its stdin closed.
fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("XDG_STATE_HOME", home)
        .env_remove("XDG_CONFIG_DIRS")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A configuration in `home` of one account `a`, its store under `home`.
///
/// Its IMAP source sits on a port bound then dropped, so closed when the run
/// connects.
fn closed_port_account(home: &Path) -> String {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let store = home.join("store").display().to_string();
    let toml = format!(
        "[accounts.a]\n\
         imap.server = \"imap://127.0.0.1:{port}\"\n\
         imap.sasl.plain.username = \"a\"\n\
         imap.sasl.plain.password.raw = \"a\"\n\
         store.root = {store:?}\n"
    );
    let path = home.join("config.toml");
    fs::write(&path, toml).unwrap();
    path.display().to_string()
}

#[test]
fn a_refused_command_line_under_json_prints_the_usage_failure_object() {
    let home = TempDir::new().unwrap();
    let output = run(home.path(), &["--json", "sync", "--nope"]);
    let stdout = text(&output.stdout);

    assert_eq!(output.status.code(), Some(2), "{stdout}");
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let error = json["error"].as_str().unwrap();
    assert_eq!(json["code"], "usage-invalid", "{json}");
    assert!(error.starts_with("unexpected argument '--nope'"), "{error}");
    assert_eq!(json["sources"], Value::Array(Vec::new()), "{json}");
    assert_eq!(json["backtrace"], Value::Null, "{json}");
    assert!(output.stderr.is_empty(), "{}", text(&output.stderr));
}

#[test]
fn a_refused_command_line_without_json_is_clap_rendering_on_stderr() {
    let home = TempDir::new().unwrap();
    let output = run(home.path(), &["sync", "--nope"]);
    let stderr = text(&output.stderr);

    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
    assert!(stderr.starts_with("error: unexpected argument"), "{stderr}");
}

/// A `--json` after `--` is a positional, not the flag.
#[test]
fn a_json_word_after_the_separator_leaves_the_refusal_on_stderr() {
    let home = TempDir::new().unwrap();
    let output = run(home.path(), &["sync", "--nope", "--", "--json"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "{}", text(&output.stdout));
    assert!(!output.stderr.is_empty());
}

/// `completion`, `manual` and `json-schema` with no argument are a refused
/// command line: exit 2, coded `usage-invalid` under `--json`.
#[test]
fn a_standard_command_without_its_argument_is_a_refused_command_line() {
    let home = TempDir::new().unwrap();

    for command in ["completion", "manual", "json-schema"] {
        let output = run(home.path(), &["--json", command]);
        let stdout = text(&output.stdout);

        assert_eq!(output.status.code(), Some(2), "{command}: {stdout}");
        let json: Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(json["code"], "usage-invalid", "{command}: {json}");
        assert!(stdout.contains("--dir"), "{command}: {stdout}");
    }
}

/// A bare `neverest --json` is a refused command line, never the help a
/// JSON reader cannot parse, even with a configuration.
#[test]
fn a_bare_invocation_under_json_is_a_refused_command_line() {
    let home = TempDir::new().unwrap();
    let config = closed_port_account(home.path());
    let output = run(home.path(), &["--json", "-c", &config]);
    let stdout = text(&output.stdout);

    assert_eq!(output.status.code(), Some(2), "{stdout}");
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["code"], "usage-invalid", "{json}");
}

/// A failed check exits 3 with its report, pimalaya-cli's account check: a
/// cleartext endpoint's row carries no TLS. Off a terminal its progress
/// lines are left out, so a redirected check holds its report alone.
#[test]
fn a_failed_check_prints_its_report_and_exits_3() {
    let home = TempDir::new().unwrap();
    let config = closed_port_account(home.path());
    let output = run(home.path(), &["--json", "-c", &config, "check"]);
    let stdout = text(&output.stdout);

    assert_eq!(output.status.code(), Some(3), "{stdout}");
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let source = &json["accounts"][0]["backends"][0];
    assert_eq!(source["backend"], "imap", "{json}");
    assert_eq!(source["ok"], false, "{json}");
    assert!(source["error"].is_string(), "{json}");
    assert!(source["tls"].is_null(), "{json}");
    assert_eq!(source["source"], "imap", "{json}");
    assert_eq!(stdout.lines().count(), 1, "{stdout}");

    let stderr = text(&output.stderr);
    assert!(!stderr.contains("Source imap failed"), "{stderr}");

    let output = run(home.path(), &["-c", &config, "check"]);
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(3), "{stdout}");
    assert!(!stdout.contains("Source imap failed"), "{stdout}");
    assert!(!stdout.contains("Resolved credentials"), "{stdout}");
    assert!(stdout.starts_with("Account:  a\n"), "{stdout}");
    assert!(stdout.contains("imap:     FAIL ("), "{stdout}");
    assert!(!stdout.contains('\u{1b}'), "{stdout}");
}

/// An account `init` could not open carries its failure as every row does.
///
/// The outermost message in `error`, its causes in `sources`, and the
/// endpoints it opened under `opened`.
#[test]
fn a_failed_init_carries_its_causes_apart() {
    let home = TempDir::new().unwrap();
    let config = closed_port_account(home.path());
    let output = run(home.path(), &["--json", "-c", &config, "init"]);
    let stdout = text(&output.stdout);

    assert_eq!(output.status.code(), Some(4), "{stdout}");
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let account = &json["accounts"][0];
    assert_eq!(account["error"], "Initialize imap", "{json}");
    let sources = account["sources"].as_array().expect("its causes");
    assert!(!sources.is_empty(), "{json}");
    assert_eq!(account["opened"], Value::Array(Vec::new()), "{json}");
}

#[test]
fn an_undrained_account_prints_its_report_and_exits_4() {
    let home = TempDir::new().unwrap();
    let config = closed_port_account(home.path());
    let output = run(
        home.path(),
        &["--json", "-c", &config, "sync", "--queue-only"],
    );
    let stdout = text(&output.stdout);

    assert_eq!(output.status.code(), Some(4), "{stdout}");
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let account = &json["queue"][0];
    assert_eq!(account["account"], "a", "{json}");
    assert!(
        account["error"].as_str().unwrap().contains("init"),
        "{json}"
    );
    assert_eq!(account["code"], "account-uninitialized", "{json}");
}

/// A termination signal while a sync waits for a store another run holds
/// stops the wait and prints the result, the account interrupted, exit 4.
#[cfg(unix)]
#[test]
fn a_signal_during_the_lock_wait_prints_the_interrupted_result() {
    use std::os::unix::fs::PermissionsExt;

    let home = TempDir::new().unwrap();
    let config = closed_port_account(home.path());
    let store = home.path().join("store");
    let local = store.join("local");
    fs::create_dir_all(&local).unwrap();
    fs::set_permissions(&local, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(store.join("pimdir.db"), b"").unwrap();
    let lock = fs::File::create(local.join("sync.lock")).unwrap();
    lock.set_permissions(fs::Permissions::from_mode(0o600))
        .unwrap();
    lock.try_lock().unwrap();

    let child = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["--json", "-c", &config, "sync"])
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path())
        .env("XDG_STATE_HOME", home.path())
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_secs(2));
    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());

    let output = child.wait_with_output().unwrap();
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(4), "{stdout}");
    let json: Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    let account = &json["accounts"][0];
    assert_eq!(account["account"], "a", "{json}");
    assert_eq!(
        account["error"], "Interrupted before the account ran",
        "{json}"
    );
}
