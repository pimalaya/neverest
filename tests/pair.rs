//! # Pair report
//!
//! A CardDAV source and target on two fake servers, the binary run over
//! them: what the report and the exit status say when one side fails.

#![cfg(feature = "carddav")]

mod fakedav;

use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

use serde_json::Value;
use tempfile::TempDir;

use fakedav::{FakeDav, Faults};

/// A vCard 4.0 named `uid`.
fn card(uid: &str) -> String {
    format!("BEGIN:VCARD\r\nVERSION:4.0\r\nUID:{uid}\r\nFN:{uid}\r\nEND:VCARD\r\n")
}

/// Writes an account `pair`, `src` syncing with the target `dst`, its store
/// under `home`, and returns the configuration path.
fn pair(home: &Path, src: &FakeDav, dst: &FakeDav, target_extra: &str) -> String {
    let store = home.join("store").display().to_string();
    let toml = format!(
        "[accounts.pair]\n\
         store.root = {store:?}\n\
         sources.src.carddav.home = {src:?}\n\
         sources.src.carddav.auth.basic.username = \"u\"\n\
         sources.src.carddav.auth.basic.password.raw = \"p\"\n\
         targets.dst.carddav.home = {dst:?}\n\
         targets.dst.carddav.auth.basic.username = \"u\"\n\
         targets.dst.carddav.auth.basic.password.raw = \"p\"\n\
         {target_extra}",
        src = src.home,
        dst = dst.home,
    );
    let path = home.join("config.toml");
    fs::write(&path, toml).unwrap();
    path.display().to_string()
}

/// Runs `neverest -c <config> --json sync <args>` and returns its exit
/// status and the account's report.
fn sync(home: &Path, config: &str, args: &[&str]) -> (i32, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", config, "--json", "sync"])
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("XDG_STATE_HOME", home)
        .env_remove("XDG_CONFIG_DIRS")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: Value = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value.get("accounts").is_some())
        .unwrap_or_else(|| panic!("no result in {stdout}"));
    let report = result["accounts"][0]["report"].clone();
    (output.status.code().unwrap(), report)
}

/// Two fake servers, the source holding one card, synced once.
fn synced_pair(home: &TempDir, target_extra: &str) -> (FakeDav, FakeDav, String) {
    let src = FakeDav::start();
    let dst = FakeDav::start();
    src.put("a.vcf", &card("a"));
    let config = pair(home.path(), &src, &dst, target_extra);

    let (code, report) = sync(home.path(), &config, &["--init"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(dst.names(), ["a.vcf"], "{report}");

    (src, dst, config)
}

#[test]
fn a_pair_collection_that_fails_is_reported_and_the_run_is_incomplete() {
    let home = tempfile::tempdir().unwrap();
    let (src, dst, config) = synced_pair(&home, "");

    let puts = dst.requests("PUT").len();
    src.put("b.vcf", &card("b"));
    dst.set_faults(Faults {
        sync_status: Some("500 Internal Server Error"),
        ..Faults::default()
    });

    let (code, report) = sync(home.path(), &config, &[]);
    assert_eq!(code, 4, "{report}");

    let failed: Vec<&Value> = report["collectionPatch"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["hunk"]["kind"] == "sync")
        .collect();
    assert_eq!(failed.len(), 1, "{report}");
    assert_eq!(failed[0]["hunk"]["source"], "src", "{report}");
    assert_eq!(failed[0]["hunk"]["target"], "dst", "{report}");
    assert_eq!(failed[0]["hunk"]["collection"], "book", "{report}");
    assert!(failed[0]["error"].is_string(), "{report}");
    assert_eq!(dst.requests("PUT").len(), puts, "nothing pushed: {report}");
}

/// A one-source CardDAV account over `dav`, its store under `home`.
fn solo(home: &Path, dav: &FakeDav) -> String {
    let store = home.join("store").display().to_string();
    let toml = format!(
        "[accounts.pair]\n\
         store.root = {store:?}\n\
         carddav.home = {home:?}\n\
         carddav.auth.basic.username = \"u\"\n\
         carddav.auth.basic.password.raw = \"p\"\n",
        home = dav.home,
    );
    let path = home.join("config.toml");
    fs::write(&path, toml).unwrap();
    path.display().to_string()
}

#[test]
fn a_listing_the_server_cuts_short_is_reported_on_every_run() {
    for no_sync in [false, true] {
        let home = tempfile::tempdir().unwrap();
        let dav = FakeDav::start();
        for uid in ["a", "b", "c"] {
            dav.put(&format!("{uid}.vcf"), &card(uid));
        }
        dav.set_faults(Faults {
            truncate: Some(1),
            no_sync,
            ..Faults::default()
        });
        let config = solo(home.path(), &dav);

        for run in [&["--init"][..], &[]] {
            let (code, report) = sync(home.path(), &config, run);
            assert_eq!(code, 4, "no_sync {no_sync}, {run:?}: {report}");
            let failures = report["failures"].as_array().unwrap();
            assert_eq!(failures.len(), 1, "{report}");
            assert_eq!(failures[0]["step"], "listing", "{report}");
            assert_eq!(failures[0]["collection"], "book", "{report}");
        }
    }
}
