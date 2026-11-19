//! # Endpoints
//!
//! An account with two CardDAV endpoints, against a local Radicale
//! (tests/radicale.sh) holding two principals. Ignored by default.
//!
//! Two servers reconciled through the store, the mirror and migration case:
//! each endpoint agrees with its own server and only the pair disagrees, so
//! the run merges what nobody disagreed about and parks the rest, unless
//! `one-way = true` answers the question the others ask a person.
//!
//! An account naming a source and a target keys the store under the source's
//! name: an address book reads as `a/<name>`, not the `carddav/<name>` of
//! tests/carddav.rs.
//!
//! Each test owns its address book on both principals, yet they run one at a
//! time: every run lists both homes, and Radicale answers 500 to a listing a
//! sibling's book delete cuts through.
//!
//! Start the server and run with:
//! ```sh
//! ./tests/radicale.sh
//! cargo test --features carddav --test endpoints -- --ignored
//! ```

mod common;

use std::{
    fs,
    path::Path,
    process::Command,
    sync::{Mutex, MutexGuard, PoisonError},
};

use io_pimdir::client::reader::PimdirItem;

use crate::common::{link_ids, none};

const DAV: &str = "http://127.0.0.1:5232";
/// The address book the divergence test owns on both principals.
const MERGE_BOOK: &str = "merge";
/// The address book the binding test owns on both principals.
const BIND_BOOK: &str = "bind";
/// The address book the ancestor-less divergence test owns on both principals.
const SEED_BOOK: &str = "seed";
/// The address book the collection lifecycle test owns on both principals.
const LIFECYCLE_BOOK: &str = "lifecycle";
/// The address book the one-way test owns on both principals.
const AUTHORITY_BOOK: &str = "authority";
/// The source endpoint's principal, whose password is its own name.
const SOURCE: &str = "test";
/// The target endpoint's principal.
const TARGET: &str = "test2";

/// Held by each test for its whole run.
static SERVER: Mutex<()> = Mutex::new(());

/// Waits for the server, whatever a sibling that panicked left.
fn serialized() -> MutexGuard<'static, ()> {
    SERVER.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A card carrying one phone number, addressed on the server by `<uid>.vcf`.
fn card(uid: &str, tel: &str) -> String {
    format!(
        "BEGIN:VCARD\r\n\
         VERSION:4.0\r\n\
         UID:{uid}\r\n\
         FN:Jane Doe\r\n\
         TEL:{tel}\r\n\
         END:VCARD\r\n",
    )
}

/// The same card with a note beside the phone number, so each endpoint can
/// change a field the other left alone.
fn noted_card(uid: &str, tel: &str, note: &str) -> String {
    format!(
        "BEGIN:VCARD\r\n\
         VERSION:4.0\r\n\
         UID:{uid}\r\n\
         FN:Jane Doe\r\n\
         TEL:{tel}\r\n\
         NOTE:{note}\r\n\
         END:VCARD\r\n",
    )
}

/// A card changed on both endpoints merges, or parks, and never overwrites.
///
/// Disjoint fields merge onto both; the same field parks (exit 3), each
/// endpoint keeping its edit over any number of reruns, until the conflict
/// commands settle it.
#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_card_changed_on_both_endpoints_merges_or_parks_and_never_overwrites() {
    let _server = serialized();
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account()).unwrap();

    create_book(root, SOURCE, MERGE_BOOK);
    create_book(root, TARGET, MERGE_BOOK);

    put(
        root,
        SOURCE,
        MERGE_BOOK,
        "card-1",
        &noted_card("card-1", "+1", "old"),
    );
    put(root, SOURCE, MERGE_BOOK, "card-2", &card("card-2", "+1"));

    neverest(&["init", "-a", "div"], &config, &state, 0);
    sync(&config, &state, MERGE_BOOK, 0);
    assert_eq!(
        get(SOURCE, MERGE_BOOK, "card-2"),
        get(TARGET, MERGE_BOOK, "card-2"),
        "the seed crossed",
    );

    put(
        root,
        SOURCE,
        MERGE_BOOK,
        "card-1",
        &noted_card("card-1", "+2", "old"),
    );
    put(
        root,
        TARGET,
        MERGE_BOOK,
        "card-1",
        &noted_card("card-1", "+1", "new"),
    );
    put(root, SOURCE, MERGE_BOOK, "card-2", &card("card-2", "+2"));
    put(root, TARGET, MERGE_BOOK, "card-2", &card("card-2", "+3"));

    let report = sync(&config, &state, MERGE_BOOK, 3);
    assert!(
        report.contains(r#""outstandingConflicts":1"#),
        "the divergence is parked and counted, not resolved; report was:\n{report}",
    );
    assert!(
        report.contains(r#""id":"card-2.vcf""#),
        "the parked item is named; report was:\n{report}",
    );

    for side in [SOURCE, TARGET] {
        let merged = get(side, MERGE_BOOK, "card-1");
        assert!(
            merged.contains("TEL:+2") && merged.contains("NOTE:new"),
            "{side} holds the merged card-1; it held:\n{merged}",
        );
    }

    assert!(
        get(SOURCE, MERGE_BOOK, "card-2").contains("TEL:+2"),
        "the source keeps its own card-2 edit",
    );
    assert!(
        get(TARGET, MERGE_BOOK, "card-2").contains("TEL:+3"),
        "the target keeps its own card-2 edit",
    );

    let report = sync(&config, &state, MERGE_BOOK, 3);
    assert!(
        !report.contains(r#""kind":"update""#),
        "a rerun pushes neither body over the other; report was:\n{report}",
    );
    assert!(
        get(SOURCE, MERGE_BOOK, "card-2").contains("TEL:+2")
            && get(TARGET, MERGE_BOOK, "card-2").contains("TEL:+3"),
        "both edits survive a rerun",
    );

    let listed = neverest(
        &["conflict", "list", "-a", "div", "--json"],
        &config,
        &state,
        0,
    );
    assert!(
        listed.contains("card-2.vcf"),
        "the divergence is listable; listing was:\n{listed}",
    );
}

/// `one-way = true` answers a real divergence instead of parking it.
///
/// The source wins: the target's edit is overwritten in the same run (the
/// deciding body is hydrated before the pushes), no conflict is counted, the
/// run exits 0, and the next one has nothing left to carry.
#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_one_way_account_overwrites_the_target_instead_of_parking_the_divergence() {
    let _server = serialized();
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, format!("{}one-way = true\n", account())).unwrap();

    create_book(root, SOURCE, AUTHORITY_BOOK);
    create_book(root, TARGET, AUTHORITY_BOOK);

    put(
        root,
        SOURCE,
        AUTHORITY_BOOK,
        "card-1",
        &card("card-1", "+1"),
    );

    neverest(&["init", "-a", "div"], &config, &state, 0);
    sync(&config, &state, AUTHORITY_BOOK, 0);
    assert_eq!(
        get(SOURCE, AUTHORITY_BOOK, "card-1"),
        get(TARGET, AUTHORITY_BOOK, "card-1"),
        "the seed crossed",
    );

    put(
        root,
        SOURCE,
        AUTHORITY_BOOK,
        "card-1",
        &card("card-1", "+2"),
    );
    put(
        root,
        TARGET,
        AUTHORITY_BOOK,
        "card-1",
        &card("card-1", "+3"),
    );

    let report = sync(&config, &state, AUTHORITY_BOOK, 0);
    assert!(
        report.contains(r#""outstandingConflicts":0"#),
        "an authoritative source leaves nothing waiting; report was:\n{report}",
    );
    assert!(
        none(&report, "conflicts"),
        "and names nothing parked; report was:\n{report}",
    );

    assert!(
        get(TARGET, AUTHORITY_BOOK, "card-1").contains("TEL:+2"),
        "the target takes the source's body, its own edit discarded",
    );
    assert!(
        get(SOURCE, AUTHORITY_BOOK, "card-1").contains("TEL:+2"),
        "and the source is never written back to, which is what makes it \
         authoritative",
    );

    let report = sync(&config, &state, AUTHORITY_BOOK, 0);
    assert!(
        !report.contains(r#""kind":"update""#),
        "a rerun pushes nothing; report was:\n{report}",
    );
    assert!(
        get(SOURCE, AUTHORITY_BOOK, "card-1").contains("TEL:+2")
            && get(TARGET, AUTHORITY_BOOK, "card-1").contains("TEL:+2"),
        "and both endpoints stay on the body the source decided",
    );

    let listed = neverest(
        &["conflict", "list", "-a", "div", "--json"],
        &config,
        &state,
        0,
    );
    assert!(
        !listed.contains("card-1.vcf"),
        "nothing was recorded for a person to settle; listing was:\n{listed}",
    );
}

/// A book deleted on one endpoint is deleted on the other, not recreated.
///
/// New on the source only, the first run creates it on the target, so both
/// are known to hold it; made again later under the same name, it reads as
/// new.
#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_book_deleted_on_one_endpoint_is_deleted_on_the_other_not_recreated() {
    let _server = serialized();
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account()).unwrap();

    create_book(root, SOURCE, LIFECYCLE_BOOK);
    delete_book(TARGET, LIFECYCLE_BOOK);
    put(
        root,
        SOURCE,
        LIFECYCLE_BOOK,
        "card-1",
        &card("card-1", "+1"),
    );

    neverest(&["init", "-a", "div"], &config, &state, 0);
    let report = sync(&config, &state, LIFECYCLE_BOOK, 0);
    assert!(
        report.contains(r#""kind":"create""#),
        "a book new on the source is created on the target; report was:\n{report}",
    );
    assert!(book_exists(TARGET, LIFECYCLE_BOOK));

    delete_book(TARGET, LIFECYCLE_BOOK);
    let report = sync(&config, &state, LIFECYCLE_BOOK, 0);
    assert!(
        report.contains(r#""kind":"delete""#) && !report.contains(r#""kind":"create""#),
        "a book both held and one deleted is deleted on the other; report was:\n{report}",
    );
    assert!(!book_exists(SOURCE, LIFECYCLE_BOOK));
    assert!(!book_exists(TARGET, LIFECYCLE_BOOK));

    create_book(root, SOURCE, LIFECYCLE_BOOK);
    let report = sync(&config, &state, LIFECYCLE_BOOK, 0);
    assert!(
        report.contains(r#""kind":"create""#) && !report.contains(r#""kind":"delete""#),
        "a book made again after a crossed delete is new; report was:\n{report}",
    );
    assert!(book_exists(SOURCE, LIFECYCLE_BOOK));
    assert!(book_exists(TARGET, LIFECYCLE_BOOK));
}

/// Two endpoints holding one card before the first sync bind it to one item.
///
/// The mirror or migration start: whichever endpoint the store reads first, a
/// second key minted for the second holder would leave two items neither
/// server takes.
#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn two_endpoints_already_holding_one_card_bind_it_to_a_single_item() {
    let _server = serialized();
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account()).unwrap();

    create_book(root, SOURCE, BIND_BOOK);
    create_book(root, TARGET, BIND_BOOK);

    put(root, SOURCE, BIND_BOOK, "card-1", &card("card-1", "+1"));
    put(root, TARGET, BIND_BOOK, "card-1", &card("card-1", "+1"));

    neverest(&["init", "-a", "div"], &config, &state, 0);
    let report = sync(&config, &state, BIND_BOOK, 0);
    assert!(
        !report.contains("dup:"),
        "the second holder is bound, not minted a key of its own; report was:\n{report}",
    );
    assert!(
        none(&report, "duplicates") && none(&report, "rejected"),
        "neither endpoint is asked to take a card it already holds; report was:\n{report}",
    );

    let items = store_items(&state, BIND_BOOK);
    assert_eq!(
        link_ids(&items),
        ["card-1"],
        "one identity is one item, and only one; store held:\n{items:#?}",
    );

    let report = sync(&config, &state, BIND_BOOK, 0);
    assert!(
        report.contains(r#""itemPatch":[]"#),
        "a bound pair is quiescent; report was:\n{report}",
    );
}

/// One identity under two bodies on two unread servers parks, never
/// overwrites.
///
/// With no common base, only a person picks a winner: the run leaves both
/// servers as found and names the divergence once, and either decision then
/// reaches both endpoints with neither body lost.
#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn two_endpoints_holding_one_card_under_two_bodies_park_it_and_never_overwrite() {
    let _server = serialized();
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account()).unwrap();

    create_book(root, SOURCE, SEED_BOOK);
    create_book(root, TARGET, SEED_BOOK);

    for uid in ["card-1", "card-2"] {
        put(root, SOURCE, SEED_BOOK, uid, &card(uid, "+1"));
        put(root, TARGET, SEED_BOOK, uid, &card(uid, "+9"));
    }

    neverest(&["init", "-a", "div"], &config, &state, 0);

    let report = sync(&config, &state, SEED_BOOK, 3);
    assert!(
        report.contains(r#""outstandingConflicts":2"#),
        "both divergences are parked and counted; report was:\n{report}",
    );
    assert!(
        report.contains(r#""id":"card-1.vcf""#) && report.contains(r#""id":"card-2.vcf""#),
        "both parked items are named; report was:\n{report}",
    );
    assert!(
        !report.contains(r#""kind":"update""#),
        "no body is pushed over the other; report was:\n{report}",
    );

    for uid in ["card-1", "card-2"] {
        assert!(
            get(SOURCE, SEED_BOOK, uid).contains("TEL:+1"),
            "the source keeps its own {uid}",
        );
        assert!(
            get(TARGET, SEED_BOOK, uid).contains("TEL:+9"),
            "the target keeps its own {uid}, which is the body the run would \
             have replaced",
        );
    }

    let report = sync(&config, &state, SEED_BOOK, 3);
    assert!(
        !report.contains(r#""kind":"update""#) && none(&report, "conflicts"),
        "a rerun pushes nothing and re-announces nothing; report was:\n{report}",
    );
    assert!(
        get(TARGET, SEED_BOOK, "card-1").contains("TEL:+9"),
        "the target's body survives a rerun",
    );

    let listed = neverest(
        &["conflict", "list", "-a", "div", "--json"],
        &config,
        &state,
        0,
    );
    let local = conflict_id(&listed, "card-1.vcf");
    let remote = conflict_id(&listed, "card-2.vcf");

    neverest(
        &["conflict", "resolve", "-a", "div", &local, "--prefer-local"],
        &config,
        &state,
        0,
    );
    neverest(
        &[
            "conflict",
            "resolve",
            "-a",
            "div",
            &remote,
            "--prefer-remote",
        ],
        &config,
        &state,
        0,
    );

    sync(&config, &state, SEED_BOOK, 0);
    for side in [SOURCE, TARGET] {
        assert!(
            get(side, SEED_BOOK, "card-1").contains("TEL:+1"),
            "{side} converged on the body --prefer-local settled on",
        );
        assert!(
            get(side, SEED_BOOK, "card-2").contains("TEL:+9"),
            "{side} converged on the body --prefer-remote settled on",
        );
    }
}

/// The id `conflict list --json` gave the divergence of `handle`.
fn conflict_id(listing: &str, handle: &str) -> String {
    let listed: serde_json::Value = serde_json::from_str(listing).expect("conflict listing");

    listed["conflicts"]
        .as_array()
        .expect("a conflict array")
        .iter()
        .find(|conflict| conflict["handle"] == handle)
        .unwrap_or_else(|| panic!("no conflict for {handle} in:\n{listing}"))["id"]
        .as_str()
        .expect("a conflict id")
        .to_owned()
}

/// The account every test runs: a source and a target, one principal each.
///
/// It keeps a local copy of what crosses.
fn account() -> String {
    format!(
        "[accounts.div]\n\
         retain = true\n\
         sources.a.carddav.server = \"{DAV}/\"\n\
         sources.a.carddav.auth.basic.username = \"{SOURCE}\"\n\
         sources.a.carddav.auth.basic.password.raw = \"{SOURCE}\"\n\
         sources.a.carddav.collection.delete = true\n\
         targets.b.carddav.server = \"{DAV}/\"\n\
         targets.b.carddav.auth.basic.username = \"{TARGET}\"\n\
         targets.b.carddav.auth.basic.password.raw = \"{TARGET}\"\n\
         targets.b.carddav.collection.delete = true\n",
    )
}

/// Recreates a principal's address book, so a run starts from an empty one.
///
/// Radicale answers a member PUT into a missing collection with 409 and keeps
/// its storage between runs, so the book is deleted and made again with an
/// extended `MKCOL`.
fn create_book(root: &Path, user: &str, book: &str) {
    let output = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-X", "DELETE"])
        .args(["-u", &format!("{user}:{user}")])
        .arg(format!("{DAV}/{user}/{book}/"))
        .output()
        .expect("spawn curl delete book");
    assert!(
        output.status.success(),
        "DELETE of {user}'s address book failed",
    );

    let body = root.join(format!("mkcol-{user}.xml"));
    fs::write(
        &body,
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <D:mkcol xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:carddav\">\
             <D:set><D:prop>\
             <D:resourcetype><D:collection/><C:addressbook/></D:resourcetype>\
             <D:displayname>{book}</D:displayname>\
             </D:prop></D:set></D:mkcol>",
        ),
    )
    .unwrap();

    let output = Command::new("curl")
        .args(["-fsS", "-X", "MKCOL", "-u", &format!("{user}:{user}")])
        .args(["-H", "Content-Type: application/xml; charset=utf-8"])
        .arg("--data-binary")
        .arg(format!("@{}", body.display()))
        .arg(format!("{DAV}/{user}/{book}/"))
        .output()
        .expect("spawn curl mkcol");

    assert!(
        output.status.success(),
        "MKCOL {user}/{book} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Deletes a principal's address book, whether or not it exists.
fn delete_book(user: &str, book: &str) {
    let output = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-X", "DELETE"])
        .args(["-u", &format!("{user}:{user}")])
        .arg(format!("{DAV}/{user}/{book}/"))
        .output()
        .expect("spawn curl delete book");
    assert!(output.status.success(), "DELETE of {user}/{book} failed");
}

/// Whether a principal's address book exists on the server.
fn book_exists(user: &str, book: &str) -> bool {
    let output = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-w", "%{http_code}"])
        .args(["-X", "PROPFIND", "-H", "Depth: 0"])
        .args(["-u", &format!("{user}:{user}")])
        .arg(format!("{DAV}/{user}/{book}/"))
        .output()
        .expect("spawn curl propfind book");
    String::from_utf8_lossy(&output.stdout).trim() == "207"
}

/// Writes a card to one principal's address book.
fn put(root: &Path, user: &str, book: &str, id: &str, body: &str) {
    let file = root.join(format!("{user}-{id}.vcf"));
    fs::write(&file, body).unwrap();

    let output = Command::new("curl")
        .args(["-fsS", "-X", "PUT", "-u", &format!("{user}:{user}")])
        .args(["-H", "Content-Type: text/vcard; charset=utf-8"])
        .arg("--data-binary")
        .arg(format!("@{}", file.display()))
        .arg(format!("{DAV}/{user}/{book}/{id}.vcf"))
        .output()
        .expect("spawn curl put");

    assert!(
        output.status.success(),
        "PUT {user}/{book}/{id} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Reads a card back from one principal's address book.
fn get(user: &str, book: &str, id: &str) -> String {
    let output = Command::new("curl")
        .args(["-fsS", "-u", &format!("{user}:{user}")])
        .arg(format!("{DAV}/{user}/{book}/{id}.vcf"))
        .output()
        .expect("spawn curl get");

    assert!(
        output.status.success(),
        "GET {user}/{book}/{id} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Syncs the account, narrowed to one address book.
fn sync(config: &Path, state: &Path, book: &str, code: i32) -> String {
    neverest(
        &["sync", "-a", "div", "-m", book, "--json"],
        config,
        state,
        code,
    )
}

/// The store's live items for an address book, with their summaries.
fn store_items(state: &Path, book: &str) -> Vec<PimdirItem> {
    common::store_items(&state.join("neverest"), &format!("div/a/{book}"))
}

/// Runs neverest and checks it ended on `code`.
///
/// 3 is a run that reconciled and left a decision waiting.
fn neverest(args: &[&str], config: &Path, state: &Path, code: i32) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(args)
        .env("XDG_STATE_HOME", state)
        .output()
        .expect("spawn neverest");

    assert_eq!(
        output.status.code(),
        Some(code),
        "`neverest {}` ended on the wrong code:\n--- stdout ---\n{}\n--- stderr ---\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8_lossy(&output.stdout).into_owned()
}
