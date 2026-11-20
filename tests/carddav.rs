//! # CardDAV
//!
//! End-to-end CardDAV test against a local Radicale (tests/radicale.sh).
//! Ignored by default.
//!
//! Mail bodies are immutable, so this is where revisions (ETags, conditional
//! writes), the conflict path and the retention of a mutable item meet a
//! server: two seeded cards land keyed by `UID`, a server edit is re-fetched,
//! a server delete is retained, and a run with nothing changed is quiescent.
//!
//! The three runs after it play a frontend staging an edit through
//! [`PimdirProducer`] while the server changes the same card (one source, no
//! target, `retain` on): disjoint fields merge, the same field parks until a
//! person resolves it, and an unreadable body is reported unmergeable.
//!
//! Each run owns an address book of its own and narrows the sync to it.
//!
//! Start the server and run with:
//! ```sh
//! ./tests/radicale.sh
//! cargo test --features carddav --test carddav -- --ignored --test-threads=1
//! ```

mod common;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use io_pimdir::{
    client::{
        producer::PimdirProducer,
        reader::{PimdirItem, PimdirReader},
    },
    codec::PimdirAction,
};

use crate::common::{link_ids, retained_items, summary_text};

const DAV: &str = "http://127.0.0.1:5232";
const USER: &str = "test";
const PASS: &str = "test";
/// The address book the server-side run owns.
const BOOK: &str = "contacts";
/// The address book the disjoint-edit merge owns.
const MERGE_BOOK: &str = "replica-merge";
/// The address book the same-field collision owns.
const COLLIDE_BOOK: &str = "replica-collide";
/// The address book the unreadable-body run owns.
const UNMERGEABLE_BOOK: &str = "replica-unmergeable";
/// The account the server-side run configures.
const ACCOUNT: &str = "contacts";
/// The account the three offline-replica runs configure: one source, no
/// target, so `retain` defaults on and the store is the destination.
const REPLICA: &str = "replica";

/// A card, addressed on the server by `<uid>.vcf` (the same resource name the
/// CardDAV adapter derives from a `UID`).
fn card(uid: &str, full_name: &str) -> String {
    format!(
        "BEGIN:VCARD\r\n\
         VERSION:4.0\r\n\
         UID:{uid}\r\n\
         FN:{full_name}\r\n\
         EMAIL:{uid}@example.org\r\n\
         END:VCARD\r\n",
    )
}

/// A card carrying a phone number beside a note, so one side can change a
/// field the other left alone.
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

/// A CardDAV side syncs, follows a server edit and retains a server delete.
///
/// Dry runs name what they would fetch: a DAV link id resolves only at `Full`,
/// so upgrading the probe would land every body and read "already in sync",
/// and a content change keeps the hub level, so a plan keyed on it skips it.
#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_carddav_side_syncs_edits_and_retains_a_server_delete() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account(ACCOUNT)).unwrap();

    create_book(root, BOOK);
    put(root, BOOK, "card-1", &card("card-1", "Jane Doe"));
    put(root, BOOK, "card-2", &card("card-2", "John Doe"));

    neverest(&["init", "-a", ACCOUNT], &config, &state, 0);

    let plan = neverest(
        &["sync", "-a", ACCOUNT, "-m", BOOK, "-d", "--json"],
        &config,
        &state,
        0,
    );
    assert!(
        plan.contains("card-1") && plan.contains("card-2"),
        "a first dry run names both cards; report was:\n{plan}",
    );

    sync(&config, &state, ACCOUNT, BOOK, 0);

    let items = store_items(&state, ACCOUNT, BOOK);
    let ids = link_ids(&items);
    assert!(
        ids.contains(&"card-1") && ids.contains(&"card-2"),
        "both cards landed, keyed by their UID; store held:\n{items:#?}",
    );
    assert!(
        items.iter().any(|item| summary_text(item) == "Jane Doe"),
        "the summary carries the display name; store held:\n{items:#?}",
    );

    put(root, BOOK, "card-1", &card("card-1", "Jane Rewritten"));

    let plan = neverest(
        &["sync", "-a", ACCOUNT, "-m", BOOK, "-d", "--json"],
        &config,
        &state,
        0,
    );
    assert!(
        plan.contains("card-1"),
        "a dry run names the body it would re-fetch; report was:\n{plan}",
    );

    sync(&config, &state, ACCOUNT, BOOK, 0);

    let items = store_items(&state, ACCOUNT, BOOK);
    assert!(
        items
            .iter()
            .any(|item| summary_text(item) == "Jane Rewritten"),
        "the edited body reached the store; store held:\n{items:#?}",
    );

    delete(BOOK, "card-2");
    sync(&config, &state, ACCOUNT, BOOK, 0);

    let live = store_items(&state, ACCOUNT, BOOK);
    assert!(
        !link_ids(&live).contains(&"card-2"),
        "the deleted card left the live listing; store held:\n{live:#?}",
    );
    let retained = retained_items(&store_dir(&state), &collection(ACCOUNT, BOOK));
    assert!(
        link_ids(&retained).contains(&"card-2"),
        "the deleted card is retained, not lost; retained listing:\n{retained:#?}",
    );

    let report = sync(&config, &state, ACCOUNT, BOOK, 0);
    assert!(
        !report.contains("\"card-2\""),
        "a quiescent run touches nothing; report was:\n{report}",
    );
}

/// A queued edit and a server edit of different fields merge in one run.
///
/// The frontend changes the note, the server the phone number; the merged
/// body goes back to the server, nothing left waiting for a person.
#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_queued_edit_and_a_server_edit_of_different_fields_merge_and_reach_the_server() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account(REPLICA)).unwrap();

    create_book(root, MERGE_BOOK);
    put(
        root,
        MERGE_BOOK,
        "card-1",
        &noted_card("card-1", "+1", "old"),
    );

    neverest(&["init", "-a", REPLICA], &config, &state, 0);
    sync(&config, &state, REPLICA, MERGE_BOOK, 0);

    queue_update(
        &state,
        REPLICA,
        MERGE_BOOK,
        "card-1",
        &noted_card("card-1", "+1", "new"),
    );
    put(
        root,
        MERGE_BOOK,
        "card-1",
        &noted_card("card-1", "+2", "old"),
    );

    let report = sync(&config, &state, REPLICA, MERGE_BOOK, 0);
    assert!(
        report.contains(r#""outstandingConflicts":0"#),
        "disjoint edits are not a disagreement, so nothing waits for a person; \
         report was:\n{report}",
    );

    let merged = get(MERGE_BOOK, "card-1");
    assert!(
        merged.contains("TEL:+2") && merged.contains("NOTE:new"),
        "the server holds both edits, the merged body having been pushed back; \
         it held:\n{merged}",
    );
}

/// A queued edit and a server edit of the same field park, never overwrite.
///
/// The run ends conflicted without touching the server's body, a rerun
/// changes nothing, and only a person's `--prefer-local` converges it.
#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_queued_edit_and_a_server_edit_of_the_same_field_park_until_a_person_decides() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account(REPLICA)).unwrap();

    create_book(root, COLLIDE_BOOK);
    put(
        root,
        COLLIDE_BOOK,
        "card-1",
        &noted_card("card-1", "+1", "old"),
    );

    neverest(&["init", "-a", REPLICA], &config, &state, 0);
    sync(&config, &state, REPLICA, COLLIDE_BOOK, 0);

    queue_update(
        &state,
        REPLICA,
        COLLIDE_BOOK,
        "card-1",
        &noted_card("card-1", "+2", "old"),
    );
    put(
        root,
        COLLIDE_BOOK,
        "card-1",
        &noted_card("card-1", "+3", "old"),
    );

    let report = sync(&config, &state, REPLICA, COLLIDE_BOOK, 3);
    assert!(
        report.contains(r#""outstandingConflicts":1"#),
        "the divergence is parked and counted; report was:\n{report}",
    );
    assert!(
        report.contains(r#""remoteId":"card-1.vcf""#),
        "the run names the item it parked; report was:\n{report}",
    );

    let listed = neverest(
        &["conflict", "list", "-a", REPLICA, "--json"],
        &config,
        &state,
        0,
    );
    assert!(
        listed.contains("card-1.vcf"),
        "the divergence is listable; listing was:\n{listed}",
    );

    assert!(
        get(COLLIDE_BOOK, "card-1").contains("TEL:+3"),
        "the server still holds its own body, unoverwritten",
    );

    let report = sync(&config, &state, REPLICA, COLLIDE_BOOK, 3);
    assert!(
        !report.contains(r#""kind":"update""#),
        "a rerun pushes neither body over the other; report was:\n{report}",
    );
    assert!(
        get(COLLIDE_BOOK, "card-1").contains("TEL:+3"),
        "the server's body survives a rerun",
    );

    let id = conflict_id(&listed, "card-1.vcf");
    neverest(
        &["conflict", "resolve", "-a", REPLICA, &id, "--prefer-local"],
        &config,
        &state,
        0,
    );
    sync(&config, &state, REPLICA, COLLIDE_BOOK, 0);

    let settled = get(COLLIDE_BOOK, "card-1");
    assert!(
        settled.contains("TEL:+2"),
        "the server converged on the body the person kept; it held:\n{settled}",
    );
}

/// An unreadable body parks as unmergeable, never as a same-field collision.
///
/// The queued body is the unreadable one: Radicale answers 400 to a malformed
/// PUT, and the merge's `unparsed` arm names whichever side failed anyway.
#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_body_no_parser_reads_is_reported_unmergeable_rather_than_collided() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account(REPLICA)).unwrap();

    create_book(root, UNMERGEABLE_BOOK);
    put(
        root,
        UNMERGEABLE_BOOK,
        "card-1",
        &noted_card("card-1", "+1", "old"),
    );

    neverest(&["init", "-a", REPLICA], &config, &state, 0);
    sync(&config, &state, REPLICA, UNMERGEABLE_BOOK, 0);

    queue_update(&state, REPLICA, UNMERGEABLE_BOOK, "card-1", "not a card");
    put(
        root,
        UNMERGEABLE_BOOK,
        "card-1",
        &noted_card("card-1", "+2", "old"),
    );

    let (report, logs) = run(
        &[
            "sync",
            "-a",
            REPLICA,
            "-m",
            UNMERGEABLE_BOOK,
            "--json",
            "--log-level",
            "debug",
        ],
        &config,
        &state,
        3,
    );

    assert!(
        report.contains(r#""outstandingConflicts":1"#)
            && report.contains(r#""remoteId":"card-1.vcf""#),
        "the item parks and the run names it; report was:\n{report}",
    );
    assert!(
        logs.contains("cannot merge") && logs.contains("does not parse"),
        "the run says the body cannot be read; logs were:\n{logs}",
    );
    assert!(
        !logs.contains("both sides changed"),
        "an unreadable body is not a field both sides set; logs were:\n{logs}",
    );

    assert!(
        get(UNMERGEABLE_BOOK, "card-1").contains("TEL:+2"),
        "the server keeps its own body, no unreadable one pushed over it",
    );
}

/// The account every run configures: one CardDAV endpoint in its direct form,
/// which is sugar for a single source named after the protocol.
fn account(name: &str) -> String {
    format!(
        "[accounts.{name}]\n\
         carddav.server = \"{DAV}/\"\n\
         carddav.auth.basic.username = \"{USER}\"\n\
         carddav.auth.basic.password.raw = \"{PASS}\"\n",
    )
}

/// The address book of `account` as the store keys it.
///
/// Account, source, then book; for the direct-backend sugar the source name
/// is the protocol name.
fn collection(account: &str, book: &str) -> String {
    format!("{account}/carddav/{book}")
}

/// Stages an edit of one card into the action queue, as a frontend does.
///
/// The body lands first, then the action pinning it, standing over whatever
/// the item holds; no server can produce this half of a single-endpoint
/// conflict, so the test writes it through io-pimdir directly.
fn queue_update(state: &Path, account: &str, book: &str, link_id: &str, body: &str) {
    let dir = store_dir(state);
    let collection = collection(account, book);

    let reader = PimdirReader::open(&dir).expect("open the store to read");
    let seq = reader
        .seq_for_link(&collection, link_id)
        .expect("resolve the item's public id")
        .unwrap_or_else(|| panic!("no item {link_id} in {collection}"));
    drop(reader);

    let mut producer = PimdirProducer::open(&dir, "carddav-test").expect("open the store to stage");
    producer
        .enqueue_with_body(&collection, body.as_bytes(), |object| {
            PimdirAction::Update {
                seq,
                object,
                if_match: None,
                resolves: None,
            }
        })
        .expect("stage the edit");
}

/// The id `conflict list --json` gave the divergence of `handle`, which is
/// what `conflict resolve` addresses it by.
fn conflict_id(listing: &str, handle: &str) -> String {
    let listed: serde_json::Value = serde_json::from_str(listing).expect("conflict listing");

    listed["conflicts"]
        .as_array()
        .expect("a conflict array")
        .iter()
        .find(|conflict| conflict["remoteId"] == handle)
        .unwrap_or_else(|| panic!("no conflict for {handle} in:\n{listing}"))["id"]
        .as_str()
        .expect("a conflict id")
        .to_owned()
}

/// Recreates an address book on the server, so a run starts from an empty one
/// whatever the previous run left behind.
///
/// Radicale does not create a collection on a member write (it answers the
/// PUT with a 409), and the container keeps its storage between runs, so the
/// book is deleted and made again with an explicit extended `MKCOL`.
fn create_book(root: &Path, book: &str) {
    let output = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-X", "DELETE"])
        .args(["-u", &format!("{USER}:{PASS}")])
        .arg(format!("{DAV}/{USER}/{book}/"))
        .output()
        .expect("spawn curl delete book");
    assert!(output.status.success(), "DELETE of {book} failed");

    let body = root.join(format!("mkcol-{book}.xml"));
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
        .args(["-fsS", "-X", "MKCOL", "-u", &format!("{USER}:{PASS}")])
        .args(["-H", "Content-Type: application/xml; charset=utf-8"])
        .arg("--data-binary")
        .arg(format!("@{}", body.display()))
        .arg(format!("{DAV}/{USER}/{book}/"))
        .output()
        .expect("spawn curl mkcol");

    assert!(
        output.status.success(),
        "MKCOL {book} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Writes a card to the server.
fn put(root: &Path, book: &str, id: &str, body: &str) {
    let file = root.join(format!("{book}-{id}.vcf"));
    fs::write(&file, body).unwrap();

    let output = Command::new("curl")
        .args(["-fsS", "-X", "PUT", "-u", &format!("{USER}:{PASS}")])
        .args(["-H", "Content-Type: text/vcard; charset=utf-8"])
        .arg("--data-binary")
        .arg(format!("@{}", file.display()))
        .arg(format!("{DAV}/{USER}/{book}/{id}.vcf"))
        .output()
        .expect("spawn curl put");

    assert!(
        output.status.success(),
        "PUT {book}/{id} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Reads a card back from the server.
fn get(book: &str, id: &str) -> String {
    let output = Command::new("curl")
        .args(["-fsS", "-u", &format!("{USER}:{PASS}")])
        .arg(format!("{DAV}/{USER}/{book}/{id}.vcf"))
        .output()
        .expect("spawn curl get");

    assert!(
        output.status.success(),
        "GET {book}/{id} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Deletes a card on the server.
fn delete(book: &str, id: &str) {
    let output = Command::new("curl")
        .args(["-fsS", "-X", "DELETE", "-u", &format!("{USER}:{PASS}")])
        .arg(format!("{DAV}/{USER}/{book}/{id}.vcf"))
        .output()
        .expect("spawn curl delete");

    assert!(
        output.status.success(),
        "DELETE {book}/{id} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Syncs an account, narrowed to one address book so a run never meets what
/// another seeded.
fn sync(config: &Path, state: &Path, account: &str, book: &str, code: i32) -> String {
    neverest(
        &["sync", "-a", account, "-m", book, "--json"],
        config,
        state,
        code,
    )
}

/// The store's live items for an address book, with their summaries.
fn store_items(state: &Path, account: &str, book: &str) -> Vec<PimdirItem> {
    common::store_items(&store_dir(state), &collection(account, book))
}

/// The account's store under the run's state directory.
///
/// Every account naming no root shares it.
fn store_dir(state: &Path) -> PathBuf {
    state.join("neverest")
}

/// Runs neverest and checks it ended on `code`, 2 being a run that reconciled
/// and left a decision waiting.
fn neverest(args: &[&str], config: &Path, state: &Path, code: i32) -> String {
    run(args, config, state, code).0
}

/// The same, keeping the logs: what a run says about a merge it could not run
/// is a log line rather than a report field.
fn run(args: &[&str], config: &Path, state: &Path, code: i32) -> (String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(args)
        .env("XDG_STATE_HOME", state)
        .output()
        .expect("spawn neverest");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    assert_eq!(
        output.status.code(),
        Some(code),
        "`neverest {}` ended on the wrong code:\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        args.join(" "),
    );

    (stdout, stderr)
}
