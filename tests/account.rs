//! # Account group
//!
//! The family's `account` group over the built binary: `account list`
//! reads the configuration alone, `account check` is the check, and `check`
//! its hidden former name. Runs without any server: the endpoint is a
//! closed local port.

use std::{
    fs,
    net::TcpListener,
    path::Path,
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};
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

/// A configuration in `home` of two accounts: `a`, an IMAP source on a
/// local port nothing listens on with its send channel, and `b`, a JMAP
/// source no build opens.
fn configuration(home: &Path) -> String {
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
         smtp.server = \"smtp://127.0.0.1:{port}\"\n\
         smtp.sasl.plain.username = \"a\"\n\
         smtp.sasl.plain.password.raw = \"a\"\n\
         store.root = {store:?}\n\
         [accounts.b]\n\
         jmap.server = \"https://jmap.example.org\"\n\
         jmap.auth.bearer.token.raw = \"token\"\n\
         store.root = {store:?}\n"
    );
    let path = home.join("config.toml");
    fs::write(&path, toml).unwrap();
    path.display().to_string()
}

#[test]
fn account_list_prints_the_shared_rows_reaching_nothing() {
    let home = TempDir::new().unwrap();
    let config = configuration(home.path());
    let output = run(home.path(), &["--json", "-c", &config, "account", "list"]);
    let stdout = text(&output.stdout);

    assert_eq!(output.status.code(), Some(0), "{stdout}");
    let json: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        json,
        json!({"accounts": [
            {"name": "a", "default": true, "backends": ["imap", "smtp"], "notBuilt": []},
            {"name": "b", "default": true, "backends": [], "notBuilt": ["jmap"]},
        ]}),
    );

    let output = run(home.path(), &["-a", "b", "-c", &config, "account", "list"]);
    let stdout = text(&output.stdout);
    assert_eq!(
        stdout.trim_end(),
        "b  jmap (not built)  default",
        "{stdout}"
    );
}

/// `account ls` is `account list`, as in every family binary, and the
/// group names the two commands it holds.
#[test]
fn account_ls_lists_and_the_group_names_its_commands() {
    let home = TempDir::new().unwrap();
    let config = configuration(home.path());
    let list = run(home.path(), &["--json", "-c", &config, "account", "list"]);
    let ls = run(home.path(), &["--json", "-c", &config, "account", "ls"]);

    assert_eq!(ls.status.code(), Some(0), "{}", text(&ls.stderr));
    assert_eq!(text(&ls.stdout), text(&list.stdout));

    let output = run(home.path(), &["account", "--help"]);
    let help = text(&output.stdout);
    assert!(
        help.starts_with(
            "List and check the configured accounts.\n\n\
             An account is a named group of sources over one pimdir store"
        ),
        "{help}"
    );

    let output = run(home.path(), &["account", "-h"]);
    let help = text(&output.stdout);
    assert!(
        help.contains("  list   List all accounts declared in the configuration [alias: ls]\n"),
        "{help}"
    );
}

#[test]
fn account_check_is_the_check_and_check_its_hidden_name() {
    let home = TempDir::new().unwrap();
    let config = configuration(home.path());

    let mut reports = Vec::new();
    for command in [&["account", "check"][..], &["check"]] {
        let mut args = vec!["--json", "-a", "a", "-c", &config];
        args.extend(command);
        let output = run(home.path(), &args);
        let stdout = text(&output.stdout);

        assert_eq!(output.status.code(), Some(3), "{command:?}: {stdout}");
        let json: Value = serde_json::from_str(&stdout).unwrap();
        let source = &json["accounts"][0]["sources"][0];
        assert_eq!(source["backend"], "imap", "{json}");
        assert_eq!(source["ok"], false, "{json}");
        assert_eq!(source["smtp"]["ok"], false, "{json}");
        reports.push(json["accounts"][0]["mode"].clone());
    }
    assert_eq!(reports[0], reports[1]);

    let output = run(home.path(), &["--help"]);
    let help = text(&output.stdout);
    assert!(help.contains("  account "), "{help}");
    assert!(!help.contains("  check "), "{help}");
}
