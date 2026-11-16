//! # First run
//!
//! The built binary run with and without a configuration, its stdin closed
//! and its stderr a pipe, so it answers as a script sees it: the help when a
//! configuration exists, the missing configuration otherwise, never the
//! welcome, which only a person at a terminal meets. Runs without any
//! server.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use serde_json::Value;
use tempfile::TempDir;

/// Runs the binary with `args` in `home`, which holds no configuration of
/// its own.
fn run(home: &Path, args: &[&str]) -> Output {
    run_with(home, args, &[])
}

/// Runs the binary as [`run`] does, with the environment variables `vars`
/// set.
fn run_with(home: &Path, args: &[&str], vars: &[(&str, &Path)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("XDG_STATE_HOME", home)
        .env_remove("XDG_CONFIG_DIRS")
        .env_remove("RUST_LOG")
        .envs(vars.iter().copied())
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

/// A home and the path of a configuration file in it holding `toml`, or
/// none when `toml` is `None`.
fn home(toml: Option<&str>) -> (TempDir, PathBuf) {
    let home = TempDir::new().unwrap();
    let path = home.path().join("config.toml");

    if let Some(toml) = toml {
        fs::write(&path, toml).unwrap();
    }

    (home, path)
}

/// An account syncing into a store under `home`, which no command run here
/// initializes: nothing connects.
fn account(home: &Path, name: &str) -> String {
    let store = home.join("store").display().to_string();
    format!(
        "[accounts.{name}]\n\
         imap.server = \"imaps://127.0.0.1:1\"\n\
         store.root = {store:?}\n"
    )
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Asserts `output` is the missing configuration at `path`, pointing at
/// the sample, with no welcome: stderr is no terminal.
fn assert_missing(output: &Output, path: &Path) {
    let (stdout, stderr) = (text(&output.stdout), text(&output.stderr));

    assert_eq!(output.status.code(), Some(1), "{stdout}");
    let missing = format!("No configuration found at {}", path.display());
    assert!(stdout.contains(&missing), "{stdout}");
    assert!(stdout.contains("start from the sample"), "{stdout}");
    assert!(stdout.contains("config.sample.toml"), "{stdout}");
    assert!(!stdout.contains("Usage:"), "{stdout}");
    assert!(!stderr.contains("Welcome"), "{stderr}");
}

#[test]
fn a_bare_invocation_prints_the_help_when_a_configuration_exists() {
    let (home, path) = home(Some(
        "[accounts.a]\nimap.server = \"imaps://127.0.0.1:1\"\n",
    ));
    let output = run(home.path(), &["-c", path.to_str().unwrap()]);
    let stdout = text(&output.stdout);

    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("Usage: neverest"), "{stdout}");
    assert!(stdout.contains("Report bugs to:"), "{stdout}");
}

/// A broken file is still a configuration, its error left to the first
/// command reading it.
#[test]
fn a_bare_invocation_prints_the_help_over_a_broken_configuration() {
    let (home, path) = home(Some("[accounts.a\n"));
    let output = run(home.path(), &["-c", path.to_str().unwrap()]);

    assert!(output.status.success(), "{}", text(&output.stdout));
    assert!(text(&output.stdout).contains("Usage: neverest"));
}

#[test]
fn a_bare_invocation_without_configuration_names_the_missing_path() {
    let (home, path) = home(None);
    let output = run(home.path(), &["-c", path.to_str().unwrap()]);

    assert_missing(&output, &path);
    assert!(!path.exists());
}

#[test]
fn a_command_without_configuration_names_the_default_path() {
    let (home, _) = home(None);
    let output = run(home.path(), &["sync"]);
    let path = home.path().join("neverest").join("config.toml");

    assert_missing(&output, &path);
    assert!(!path.exists());
}

#[test]
fn a_json_caller_gets_the_missing_configuration_as_the_failure_object() {
    let (home, path) = home(None);
    let args = ["--json", "-c", path.to_str().unwrap(), "check"];
    let output = run(home.path(), &args);

    assert_eq!(output.status.code(), Some(1));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let error = json["error"].as_str().unwrap();
    assert!(error.contains(&path.display().to_string()), "{error}");
    assert!(error.contains("config.sample.toml"), "{error}");
    assert!(output.stderr.is_empty(), "{}", text(&output.stderr));
}

#[test]
fn a_broken_configuration_is_an_error_rather_than_absent() {
    let (home, path) = home(Some("[accounts.a\n"));
    let output = run(home.path(), &["-c", path.to_str().unwrap(), "check"]);
    let stdout = text(&output.stdout);

    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(!stdout.contains("No configuration found"), "{stdout}");
}

#[test]
fn an_unknown_account_names_the_ones_the_configuration_holds() {
    let (home, path) = home(None);
    fs::write(
        &path,
        account(home.path(), "a") + &account(home.path(), "b"),
    )
    .unwrap();
    let args = [
        "--json",
        "-c",
        path.to_str().unwrap(),
        "-a",
        "x",
        "sync",
        "--queue-only",
    ];
    let output = run(home.path(), &args);

    assert_eq!(output.status.code(), Some(1));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let expected = "Account `x` not found, the configuration holds: a, b";
    assert_eq!(json["error"], expected, "{json}");
    assert_eq!(json["code"], "account-missing", "{json}");
}

/// `-c` is the flag every Pimalaya CLI shares: one path a flag, the first
/// the base and the next merged on top.
#[test]
fn repeated_config_paths_merge_in_order() {
    let (home, base) = home(None);
    let other = home.path().join("other.toml");
    fs::write(&base, account(home.path(), "a")).unwrap();
    fs::write(&other, account(home.path(), "b")).unwrap();
    let (base, other) = (base.to_str().unwrap(), other.to_str().unwrap());
    let output = run(
        home.path(),
        &["--json", "-c", base, "-c", other, "sync", "--queue-only"],
    );
    let stdout = text(&output.stdout);

    // NOTE: neither account is initialized, which a drain reports per
    // account and exits 4 for.
    assert_eq!(output.status.code(), Some(4), "{stdout}");
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let names: Vec<&str> = json["queue"]
        .as_array()
        .unwrap()
        .iter()
        .map(|account| account["account"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["a", "b"], "{json}");
}

#[test]
fn a_config_path_holding_a_colon_stays_whole() {
    let (home, _) = home(None);
    let path = home.path().join("a:b.toml");
    let output = run(home.path(), &["-c", path.to_str().unwrap(), "check"]);

    assert_missing(&output, &path);
}

#[test]
fn the_former_config_variable_is_not_read() {
    let (home, path) = home(None);
    fs::write(&path, account(home.path(), "a")).unwrap();
    let output = run_with(home.path(), &["check"], &[("NEVEREST_CONFIG", &path)]);
    let default = home.path().join("neverest").join("config.toml");

    assert_missing(&output, &default);
}
