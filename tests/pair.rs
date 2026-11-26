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

/// A lone source no write can reach is listed once a run too, a card removed
/// moving its sync token so the run lists it.
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
    dav.remove("a.vcf");
    let (code, report) = sync(home.path(), &config, &[]);

    assert_eq!(code, 0, "{report}");
    assert_eq!(dav.requests("REPORT").len() - listed, 1, "{report}");
}

/// `-m` and `-x` also name a collection by its display name when no id
/// matches, on both sides of a pair alike, the target's book listed under
/// another name; a configured filter still names ids alone.
///
/// Matched on ids alone before, `-m Friends` kept nothing and `-x Friends`
/// dropped nothing.
#[test]
fn a_command_line_filter_names_a_collection_by_its_display_name() {
    let runs: [(&[&str], &str, bool); 4] = [
        (&["-m", "friends"], "", true),
        (&["-x", "Friends"], "", false),
        (&["-m", "book"], "", true),
        (
            &[],
            "sources.src.carddav.collection.filter.include = [\"Friends\"]\n",
            false,
        ),
    ];
    for (args, extra, crosses) in runs {
        let home = tempfile::tempdir().unwrap();
        let src = FakeDav::start();
        let dst = FakeDav::start();
        src.set_name("Friends");
        src.put("a.vcf", &card("a"));
        let config = pair(home.path(), &src, &dst, extra);

        let args = [&["--init"], args].concat();
        let (code, report) = sync(home.path(), &config, &args);

        assert_eq!(code, 0, "{args:?}: {report}");
        let expected: &[&str] = if crosses { &["a.vcf"] } else { &[] };
        assert_eq!(dst.names(), expected, "{args:?} {extra}: {report}");
    }
}

/// A delete queued while the server edits the card is overruled by the
/// edit (pimdir SYNC §5): the card stays, and the run says so in its JSON
/// and its text, the queue's row applied.
///
/// The run read "applied 1 queued action(s)" before, the card back with no
/// word (the live night of 2026-11-23).
#[test]
fn a_delete_a_server_edit_overrules_is_reported() {
    let (home, dav, config) = delete_beside_a_server_edit();
    let (code, report) = sync(home.path(), &config, &[]);

    assert_eq!(code, 0, "{report}");
    assert_eq!(dav.names(), ["a.vcf"], "the edit wins: {report}");
    let overruled: Vec<&Value> = report["itemPatch"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["kind"] == "delete-overruled")
        .collect();
    assert_eq!(overruled.len(), 1, "{report}");
    assert_eq!(overruled[0]["side"], "carddav", "{report}");
    assert_eq!(overruled[0]["collection"], "book", "{report}");
    assert_eq!(overruled[0]["id"], "a", "named by its UID: {report}");
    let (code, report) = sync(home.path(), &config, &[]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["itemPatch"], serde_json::json!([]), "said once");

    let (home, _dav, config) = delete_beside_a_server_edit();
    let text = sync_text(home.path(), &config);
    let line = "delete of item a in book overruled by a server edit on carddav";
    assert!(text.contains(line), "{text}");
}

/// A book deleted on the server is retired from the store and reported, a
/// `retire` collection hunk, once; one a parked row anchors on stays,
/// reported waiting for a person (exit 3), until the row goes.
///
/// It stayed in the store with its cards, the report empty, before (the
/// live night of 2026-11-23).
#[test]
fn a_book_deleted_on_the_server_is_retired_and_reported() {
    use io_pimdir::{
        client::{producer::PimdirProducer, reader::PimdirReader},
        codec::PimdirAction,
        object::PimdirHash,
    };

    let held = |home: &Path| {
        PimdirReader::open(home.join("store"))
            .unwrap()
            .list_collections()
            .unwrap()
            .into_iter()
            .any(|collection| collection.id == "pair/carddav/book")
    };
    let retired = |report: &Value| {
        report["collectionPatch"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|hunk| hunk["kind"] == "retire")
            .count()
    };
    let synced = || {
        let home = tempfile::tempdir().unwrap();
        let dav = FakeDav::start();
        dav.add_spare();
        dav.put("a.vcf", &card("a"));
        let config = solo(home.path(), &dav);
        let (code, report) = sync(home.path(), &config, &["--init"]);
        assert_eq!(code, 0, "{report}");
        assert!(held(home.path()), "{report}");
        (home, dav, config)
    };

    let (home, dav, config) = synced();
    dav.delete_book();
    let (code, report) = sync(home.path(), &config, &[]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(retired(&report), 1, "{report}");
    let hunk = &report["collectionPatch"][0];
    assert_eq!(hunk["side"], "carddav", "{report}");
    assert_eq!(hunk["collection"], "book", "{report}");
    assert!(!held(home.path()), "the book is retired: {report}");
    let (code, report) = sync(home.path(), &config, &[]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["collectionPatch"], serde_json::json!([]), "once");

    let (home, dav, config) = synced();
    let store = home.path().join("store");
    let seq = PimdirReader::open(&store)
        .unwrap()
        .list_items("pair/carddav/book", None, 10)
        .unwrap()[0]
        .seq;
    let stale = PimdirAction::Remove {
        seq,
        if_match: Some(PimdirHash("a".repeat(52))),
        resolves: None,
    };
    PimdirProducer::open(&store, "test-frontend")
        .unwrap()
        .enqueue("pair/carddav/book", &stale, None)
        .unwrap();
    dav.delete_book();
    let (code, report) = sync(home.path(), &config, &[]);
    assert_eq!(code, 3, "{report}");
    assert!(held(home.path()), "kept while the row anchors: {report}");
    let kept: Vec<&Value> = report["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["kind"] == "collection-patch" && entry["hunk"]["kind"] == "retire")
        .collect();
    assert_eq!(kept.len(), 1, "{report}");
}

/// A one-source CardDAV account synced once, a delete of its one card
/// queued, and the card edited on the server since.
fn delete_beside_a_server_edit() -> (TempDir, FakeDav, String) {
    use io_pimdir::{
        client::{producer::PimdirProducer, reader::PimdirReader},
        codec::PimdirAction,
    };

    let home = tempfile::tempdir().unwrap();
    let dav = FakeDav::start();
    dav.put("a.vcf", &card("a"));
    let config = solo(home.path(), &dav);
    let (code, report) = sync(home.path(), &config, &["--init"]);
    assert_eq!(code, 0, "{report}");

    let store = home.path().join("store");
    let items = PimdirReader::open(&store)
        .unwrap()
        .list_items("pair/carddav/book", None, 10)
        .unwrap();
    let remove = PimdirAction::Remove {
        seq: items[0].seq,
        if_match: None,
        resolves: None,
    };
    PimdirProducer::open(&store, "test-frontend")
        .unwrap()
        .enqueue("pair/carddav/book", &remove, None)
        .unwrap();
    dav.put("a.vcf", &card("a").replace("FN:a", "FN:a edited"));

    (home, dav, config)
}

/// Runs `neverest -c <config> sync` in text mode, answering its output.
fn sync_text(home: &Path, config: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", config, "sync"])
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("XDG_STATE_HOME", home)
        .env_remove("XDG_CONFIG_DIRS")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A lone source's `-m` names its collection by its display name too.
#[test]
fn a_lone_source_s_command_line_filter_names_a_collection_by_its_display_name() {
    let home = tempfile::tempdir().unwrap();
    let dav = FakeDav::start();
    dav.set_name("Friends");
    dav.put("a.vcf", &card("a"));
    let config = solo(home.path(), &dav);

    let (code, report) = sync(home.path(), &config, &["--init", "-m", "FRIENDS"]);

    assert_eq!(code, 0, "{report}");
    let fetched: Vec<&Value> = report["itemPatch"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["kind"] == "fetch")
        .collect();
    assert_eq!(fetched.len(), 1, "{report}");
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

/// A book whose sync token in the home-set listing equals its checkpoint
/// sends no `REPORT` (RFC 6578 §4): nothing changed there. A card added
/// moves the token, and the next run lists it.
#[test]
fn a_book_whose_sync_token_did_not_move_is_not_listed() {
    let home = tempfile::tempdir().unwrap();
    let dav = FakeDav::start();
    dav.put("a.vcf", &card("a"));
    let config = solo(home.path(), &dav);

    let (code, report) = sync(home.path(), &config, &["--init"]);
    assert_eq!(code, 0, "{report}");
    let reports = dav.requests("REPORT").len();

    let (code, report) = sync(home.path(), &config, &[]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(dav.requests("REPORT").len(), reports, "{report}");

    dav.put("b.vcf", &card("b"));
    let (code, report) = sync(home.path(), &config, &[]);
    assert_eq!(code, 0, "{report}");
    let synced = dav.requests("REPORT");
    assert!(
        synced[reports..]
            .iter()
            .any(|line| line.contains("/dav/book/")),
        "{synced:?}"
    );
    assert!(
        !report["itemPatch"].as_array().unwrap().is_empty(),
        "{report}"
    );
}

/// The UIDs of the live cards a lone source's store holds.
fn store_names(home: &Path) -> Vec<String> {
    use io_pimdir::client::reader::PimdirReader;

    let reader = PimdirReader::open(home.join("store")).unwrap();
    let mut names = Vec::new();
    for collection in reader.list_collections().unwrap() {
        if collection.kind != "text/vcard" {
            continue;
        }
        for item in reader.list_items(&collection.id, None, 100).unwrap() {
            if item.retention.is_none() {
                names.push(item.link_id.0);
            }
        }
    }
    names.sort();
    names
}

/// A delta naming members of another book keeps them out of this one's.
///
/// Stalwart lists a sibling book's tombstone in a book's `sync-collection`:
/// keyed by its last segment, it pulled the deletion of the same-named card
/// this book holds, and the token moved past it.
#[test]
fn a_delta_naming_another_book_s_members_leaves_this_book_as_it_is() {
    let home = tempfile::tempdir().unwrap();
    let dav = FakeDav::start();
    dav.put("a.vcf", &card("a"));
    let config = solo(home.path(), &dav);
    let (code, report) = sync(home.path(), &config, &["--init"]);
    assert_eq!(code, 0, "{report}");

    dav.put("b.vcf", &card("b"));
    dav.set_faults(Faults {
        foreign_removed: vec!["a.vcf"],
        foreign_changed: vec!["z.vcf"],
        ..Faults::default()
    });

    for _ in 0..2 {
        let (code, report) = sync(home.path(), &config, &[]);
        assert_eq!(code, 0, "{report}");
        assert_eq!(store_names(home.path()), ["a", "b"], "{report}");
    }
}
