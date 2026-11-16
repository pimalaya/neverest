//! # Exit contract
//!
//! The built binary's exit statuses beyond success and failure: a command
//! line the grammar refuses exits 2, its failure object coded
//! `usage-invalid` under `--json`; a standard command missing its argument
//! fails like any other command; a check that printed its report with an
//! endpoint down exits 3, and a drain that left an account undrained exits
//! 4. Runs without any server: the endpoint down is a closed local port.

use std::{
    fs,
    net::TcpListener,
    path::Path,
    process::{Command, Output, Stdio},
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

/// A configuration in `home` of one account, `a`, whose IMAP source sits
/// on a local port nothing listens on, its store under `home`.
fn closed_port_account(home: &Path) -> String {
    // NOTE: bound then dropped, so the port is closed when the run connects.
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

/// `completion`, `manual` and `json-schema` with no argument fail as any
/// command does, the error on stdout, rather than printing their help.
#[test]
fn a_standard_command_without_its_argument_fails_on_stdout() {
    let home = TempDir::new().unwrap();

    for command in ["completion", "manual", "json-schema"] {
        let output = run(home.path(), &[command]);
        let stdout = text(&output.stdout);

        assert_eq!(output.status.code(), Some(1), "{command}: {stdout}");
        assert!(stdout.contains("--dir"), "{command}: {stdout}");
        assert!(!stdout.contains("Usage:"), "{command}: {stdout}");
        assert!(output.stderr.is_empty(), "{}", text(&output.stderr));
    }
}

/// A failed check exits 3 with its report; its notice is a plain line on
/// stderr in text mode and absent under `--json`.
#[test]
fn a_failed_check_prints_its_report_and_exits_3() {
    let home = TempDir::new().unwrap();
    let config = closed_port_account(home.path());
    let output = run(home.path(), &["--json", "-c", &config, "check"]);
    let stdout = text(&output.stdout);

    assert_eq!(output.status.code(), Some(3), "{stdout}");
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let source = &json["accounts"][0]["sources"][0];
    assert_eq!(source["backend"], "imap", "{json}");
    assert_eq!(source["ok"], false, "{json}");
    assert!(source["error"].is_string(), "{json}");
    assert_eq!(stdout.lines().count(), 1, "{stdout}");

    let stderr = text(&output.stderr);
    assert!(!stderr.contains("Source imap failed"), "{stderr}");

    let output = run(home.path(), &["-c", &config, "check"]);
    let stderr = text(&output.stderr);
    assert!(stderr.contains("Source imap failed ("), "{stderr}");
    assert!(!stderr.contains('\u{1b}'), "{stderr}");
}

/// An account `init` could not open carries its failure as every report
/// row does: the outermost message in `error`, its causes in `sources`,
/// and the endpoints it opened under `opened`.
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
}
