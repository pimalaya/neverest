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

    let failed: Vec<&Value> = report["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["kind"] == "collection-patch" && entry["hunk"]["kind"] == "sync")
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
            let failures: Vec<&Value> = report["errors"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry["kind"] == "failures")
                .collect();
            assert_eq!(failures.len(), 1, "{report}");
            assert_eq!(failures[0]["step"], "listing", "{report}");
            assert_eq!(failures[0]["collection"], "book", "{report}");
        }
    }
}

/// A card the source lists with a 403 row is not deleted on the target, and
/// the listing is reported incomplete on every run.
///
/// Read as a whole collection, the `PROPFIND` listing lacking the card
/// stated it removed, and the run pushed its delete.
#[test]
fn a_member_the_server_answers_with_an_error_is_not_deleted() {
    for no_sync in [false, true] {
        let home = tempfile::tempdir().unwrap();
        let (src, dst, config) = synced_pair(&home, "");
        src.put("b.vcf", &card("b"));
        let (code, report) = sync(home.path(), &config, &[]);
        assert_eq!(code, 0, "{report}");
        assert_eq!(dst.names(), ["a.vcf", "b.vcf"], "{report}");

        src.set_faults(Faults {
            refused: vec!["b.vcf"],
            no_sync,
            ..Faults::default()
        });

        for _ in 0..2 {
            let (code, report) = sync(home.path(), &config, &[]);
            assert_eq!(
                dst.names(),
                ["a.vcf", "b.vcf"],
                "no_sync {no_sync}: {report}"
            );
            assert_eq!(
                src.names(),
                ["a.vcf", "b.vcf"],
                "no_sync {no_sync}: {report}"
            );
            assert_eq!(code, 4, "no_sync {no_sync}: {report}");
            let failures: Vec<&Value> = report["errors"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry["kind"] == "failures")
                .collect();
            assert_eq!(failures.len(), 1, "{report}");
            assert_eq!(failures[0]["step"], "listing", "{report}");
            assert_eq!(failures[0]["collection"], "book", "{report}");
            let error = failures[0]["error"].as_str().unwrap();
            assert!(error.contains("b.vcf (403)"), "{report}");
        }
    }
}

#[test]
fn writes_a_target_s_rights_forbid_are_reported_held_never_applied() {
    let home = tempfile::tempdir().unwrap();
    let forbid = "targets.dst.carddav.item.create = false\n\
                  targets.dst.carddav.item.update = false\n\
                  targets.dst.carddav.item.delete = false\n";
    let (src, dst, config) = synced_pair(&home, "");
    src.put("b.vcf", &card("b"));
    let (code, _) = sync(home.path(), &config, &[]);
    assert_eq!(code, 0);
    let config = pair(home.path(), &src, &dst, forbid);

    src.remove("a.vcf");
    src.put("b.vcf", &card("b").replace("FN:b", "FN:bee"));
    src.put("c.vcf", &card("c"));
    let writes = dst.requests("PUT").len() + dst.requests("DELETE").len();

    for _ in 0..2 {
        let (code, report) = sync(home.path(), &config, &[]);
        assert_eq!(code, 0, "{report}");

        let on_dst: Vec<&Value> = report["itemPatch"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|hunk| hunk["side"] == "dst" || hunk["targetSide"] == "dst")
            .collect();
        assert!(on_dst.is_empty(), "{report}");

        let mut forbidden: Vec<(String, u64)> = report["forbidden"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                assert_eq!(row["side"], "dst", "{report}");
                let action = row["kind"].as_str().unwrap().to_string();
                (action, row["count"].as_u64().unwrap())
            })
            .collect();
        forbidden.sort();
        let expected =
            [("create", 1), ("delete", 1), ("update", 1)].map(|(a, n)| (a.to_string(), n));
        assert_eq!(forbidden, expected, "{report}");
    }

    let after = dst.requests("PUT").len() + dst.requests("DELETE").len();
    assert_eq!(after, writes, "no write reached the target");
}

/// A collection delete queued while the right was on is not performed by
/// a declaration once the configuration turned it off: it parks.
#[test]
fn a_declaration_performs_no_collection_intent_its_rights_now_refuse() {
    use io_pimdir::client::producer::{PimdirActionStatus, PimdirProducer};

    let home = tempfile::tempdir().unwrap();
    let dav = FakeDav::start();
    dav.put("a.vcf", &card("a"));
    let config = solo(home.path(), &dav);
    let granted = fs::read_to_string(&config).unwrap() + "carddav.collection.delete = true\n";
    fs::write(&config, &granted).unwrap();
    let (code, report) = sync(home.path(), &config, &["--init"]);
    assert_eq!(code, 0, "{report}");

    let store = home.path().join("store");
    let mut producer = PimdirProducer::open(&store, "neverest-tests").unwrap();
    let id = producer
        .enqueue_collection_delete("pair/carddav/book", None)
        .unwrap()
        .handle
        .id;
    fs::write(&config, granted.replace("delete = true", "delete = false")).unwrap();

    sync(home.path(), &config, &["--declare-only"]);

    assert!(
        !dav.requests("DELETE")
            .contains(&String::from("DELETE /dav/book/")),
        "the book was deleted"
    );
    let status = producer.action_status(id).unwrap();
    assert!(
        matches!(status, PimdirActionStatus::Parked { .. }),
        "{status:?}"
    );
}

/// A side no write can reach is listed once a run, by the opening round:
/// the settling rounds list only the sides they push to.
#[test]
fn a_side_nothing_writes_is_listed_once_a_run() {
    let home = tempfile::tempdir().unwrap();
    let forbid = "sources.src.carddav.item.create = false\n\
                  sources.src.carddav.item.update = false\n\
                  sources.src.carddav.item.delete = false\n";
    let (src, dst, config) = synced_pair(&home, forbid);

    let listed = src.requests("REPORT").len();
    dst.put("b.vcf", &card("b"));
    let (code, report) = sync(home.path(), &config, &[]);

    assert_eq!(code, 0, "{report}");
    assert_eq!(src.requests("REPORT").len() - listed, 1, "{report}");
    assert_eq!(src.names(), ["a.vcf"], "{report}");
}

/// A lone source no write can reach is listed once a run too.
#[test]
fn a_lone_source_nothing_writes_is_listed_once_a_run() {
    let home = tempfile::tempdir().unwrap();
    let dav = FakeDav::start();
    dav.put("a.vcf", &card("a"));
    let config = solo(home.path(), &dav);
    let mut toml = fs::read_to_string(&config).unwrap();
    toml.push_str(
        "carddav.item.create = false\n\
         carddav.item.update = false\n\
         carddav.item.delete = false\n",
    );
    fs::write(&config, toml).unwrap();

    let (code, report) = sync(home.path(), &config, &["--init"]);
    assert_eq!(code, 0, "{report}");
    let listed = dav.requests("REPORT").len();
    let (code, report) = sync(home.path(), &config, &[]);

    assert_eq!(code, 0, "{report}");
    assert_eq!(dav.requests("REPORT").len() - listed, 1, "{report}");
}

/// A target's `collection.filter` narrows what that target takes: a
/// collection it leaves out is neither created on it nor synced.
///
/// Ignored before, the filter loaded and every collection crossed.
#[test]
fn a_target_s_filter_narrows_what_it_takes() {
    let home = tempfile::tempdir().unwrap();
    let src = FakeDav::start();
    let dst = FakeDav::start();
    src.put("a.vcf", &card("a"));
    let narrow = "targets.dst.carddav.collection.filter.exclude = [\"book\"]\n";
    let config = pair(home.path(), &src, &dst, narrow);

    let (code, report) = sync(home.path(), &config, &["--init"]);

    assert_eq!(code, 0, "{report}");
    assert!(dst.names().is_empty(), "{report}");
    assert!(dst.requests("PUT").is_empty(), "{report}");
}
