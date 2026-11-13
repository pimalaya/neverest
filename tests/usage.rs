//! A command line the grammar refuses: exit 2, and under `--json` the
//! failure object coded `usage-invalid` on stdout. Runs without any server.
//!
//! The partial outcomes (3 and 4) are covered where they arise: tests/check.rs
//! for a check with an endpoint down, the sync suites for the rest.

use std::process::{Command, Output};

use serde_json::{Value, json};

fn neverest(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(args)
        .env("NEVEREST_CONFIG", "/nonexistent/neverest/config.toml")
        .output()
        .expect("spawn neverest")
}

#[test]
fn a_refused_command_line_prints_the_usage_failure_under_json() {
    let output = neverest(&["--json", "sync", "--no-such-flag"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(2), "{stdout}");

    let failure: Value = serde_json::from_str(&stdout).expect("one JSON document");
    assert_eq!(failure["code"], "usage-invalid", "{failure}");
    assert_eq!(failure["sources"], json!([]), "{failure}");
    assert_eq!(failure["backtrace"], Value::Null, "{failure}");

    let error = failure["error"].as_str().expect("an error message");
    assert!(error.contains("--no-such-flag"), "{error}");
    assert!(!error.starts_with("error:"), "{error}");
}

#[test]
fn a_refused_command_line_without_json_is_rendered_on_stderr() {
    let output = neverest(&["sync", "--no-such-flag"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--no-such-flag"), "{stderr}");
}
