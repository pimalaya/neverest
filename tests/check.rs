//! `neverest check` against a local Stalwart (server A: IMAP on :143, HTTP
//! and DAV on :8080), spawned via `tests/stalwart2.sh`. Ignored by default.
//!
//! What a caller adding an account reads from a check: the collections
//! with the default the server states, and what each source declares it
//! can do.
//!
//! Start the servers and run with:
//! ```sh
//! ./tests/stalwart2.sh
//! cargo test --test check -- --ignored
//! ```

use std::{fs, net::TcpListener, path::Path, process::Command};

use serde_json::Value;

const DAV: &str = "http://127.0.0.1:8080/dav";
const USER: &str = "test@pimalaya.org";
const PASS: &str = "P!malaya-test-2026";

#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :8080 and --ignored"]
fn a_caldav_check_names_the_default_calendar_the_scheduling_inbox_states() {
    let report = check("caldav", &format!("{DAV}/cal/"));
    let source = &report["sources"][0];
    let collections = source["collections"].as_array().expect("collections");

    // NOTE: Stalwart's scheduling inbox names the calendar it provisions.
    let defaults: Vec<&Value> = collections
        .iter()
        .filter(|collection| collection["default"] == true)
        .collect();
    assert_eq!(defaults.len(), 1, "{report}");
    assert_eq!(defaults[0]["id"], "default", "{report}");

    assert_eq!(support(source, "calendar.item.add"), "full", "{report}");
}

#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :8080 and --ignored"]
fn a_carddav_check_names_no_default_address_book() {
    let report = check("carddav", &format!("{DAV}/card/"));
    let source = &report["sources"][0];
    let collections = source["collections"].as_array().expect("collections");

    assert!(!collections.is_empty(), "{report}");
    assert!(
        collections
            .iter()
            .all(|collection| collection["default"] == false),
        "{report}"
    );
    assert_eq!(support(source, "contacts.card.add"), "full", "{report}");
}

/// Checks a one-source account of `protocol` on `server`, and returns the
/// JSON report.
fn check(protocol: &str, server: &str) -> Value {
    let tmp = tempfile::tempdir().expect("temp dir");
    let state = tmp.path().join("state");
    let config = tmp.path().join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(
        &config,
        format!(
            "[accounts.check]\n\
             {protocol}.server = \"{server}\"\n\
             {protocol}.auth.basic.username = \"{USER}\"\n\
             {protocol}.auth.basic.password.raw = \"{PASS}\"\n",
        ),
    )
    .unwrap();

    let checked = neverest(&["--json", "check", "-a", "check"], &config, &state);
    let checked: Value = serde_json::from_str(&checked).expect("one JSON document");
    checked["accounts"][0].clone()
}

/// The support a source declares for the capability `name`.
fn support<'a>(source: &'a Value, name: &str) -> &'a str {
    source["capabilities"]
        .as_array()
        .expect("capabilities")
        .iter()
        .find(|capability| capability["name"] == name)
        .and_then(|capability| capability["support"].as_str())
        .unwrap_or_else(|| panic!("no {name} declared: {source}"))
}

fn neverest(args: &[&str], config: &Path, state: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(args)
        .env("XDG_STATE_HOME", state)
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

/// A port nothing listens on.
fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// An endpoint that does not answer fails its row, not the check: every
/// endpoint is still checked and printed, and the command exits 2 rather
/// than printing a failure object. Runs without any server.
#[test]
fn a_check_names_every_endpoint_that_did_not_answer() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let state = tmp.path().join("state");
    let config = tmp.path().join("config.toml");
    fs::create_dir_all(&state).unwrap();
    let (source, target) = (closed_port(), closed_port());
    fs::write(
        &config,
        format!(
            "[accounts.check]\n\
             caldav.server = \"http://127.0.0.1:{source}/\"\n\
             caldav.auth.basic.username = \"{USER}\"\n\
             caldav.auth.basic.password.raw = \"{PASS}\"\n\
             targets.backup.caldav.server = \"http://127.0.0.1:{target}/\"\n\
             targets.backup.caldav.auth.basic.username = \"{USER}\"\n\
             targets.backup.caldav.auth.basic.password.raw = \"{PASS}\"\n",
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(["--json", "check", "-a", "check"])
        .env("XDG_STATE_HOME", &state)
        .output()
        .expect("spawn neverest");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(2), "{stdout}");

    let checked: Value = serde_json::from_str(&stdout).expect("one JSON document");
    assert_eq!(checked["orphans"], serde_json::json!([]), "{checked}");
    let report = &checked["accounts"][0];
    assert_eq!(report["account"], "check", "{checked}");
    assert_eq!(
        report["mode"],
        serde_json::json!({
            "sources": ["caldav"],
            "targets": ["backup"],
            "oneWay": false,
            "retain": false,
        }),
        "{report}"
    );

    let sources = report["sources"].as_array().expect("sources");
    let names: Vec<&str> = sources
        .iter()
        .filter_map(|s| s["source"].as_str())
        .collect();
    assert_eq!(names, ["backup", "caldav"], "{report}");
    for source in sources {
        assert_eq!(source["backend"], "caldav", "{report}");
        assert_eq!(source["ok"], false, "{report}");
        assert!(source["error"].is_string(), "{report}");
        assert_eq!(source["collections"], serde_json::json!([]), "{report}");
        assert!(source["smtp"].is_null(), "{report}");
    }
    assert!(
        !sources[1]["capabilities"].as_array().unwrap().is_empty(),
        "a source declares what it can do from its configuration: {report}"
    );
}
