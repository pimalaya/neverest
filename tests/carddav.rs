//! End-to-end CardDAV test against a local Radicale (`tests/radicale.sh`).
//! Ignored by default.
//!
//! This is the run that exercises what mail structurally cannot. Mail bodies
//! are immutable, so until a DAV backend existed the revision plumbing (ETags,
//! conditional writes), the conflict path and the retention of a mutable item
//! were only ever driven by a fake remote. Here they meet a server:
//!
//!   1. Seed two cards on the server (`curl` PUT), sync, and check the store
//!      holds both, keyed by their vCard `UID`.
//!   2. Edit one card **on the server**, sync, and check the store followed
//!      the new body (the ETag moved, so the item is re-fetched).
//!   3. Delete a card on the server, sync, and check it is **retained**, not
//!      lost: the local copy and its body survive an upstream expunge.
//!   4. Sync again with nothing changed and check the run is quiescent, which
//!      is what proves a retained row is invisible to the merge rather than
//!      re-derived on every run.
//!
//! The three runs after it are the offline replica's own conflict path: one
//! source, no target, `retain` on by default, and a **frontend** queueing an
//! edit into the store while the server independently changes the same card.
//! Nothing but a frontend produces that shape, so the test plays one, staging
//! its edit through [`PimdirProducer`] exactly as himalaya would rather than
//! through a second endpoint:
//!
//!   5. Disjoint fields merge, and the merged body reaches the server.
//!   6. The same field on both sides parks, survives a rerun untouched, and
//!      converges only once a person resolves it.
//!   7. A body no parser reads is reported unmergeable rather than counted as
//!      a collision nobody had.
//!
//! Each run owns an address book of its own and narrows the sync to it, so a
//! run never meets what another seeded.
//!
//! Start the server and run with:
//! ```sh
//! ./tests/radicale.sh
//! cargo test --features dav --test carddav -- --ignored --test-threads=1
//! ```

use std::{fs, io::Write, path::Path, process::Command};

use chrono::Utc;
use io_pimdir::{PimdirProducer, PimdirReader, codec::PimdirAction};

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

#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_carddav_side_syncs_edits_and_retains_a_server_delete() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account(ACCOUNT)).unwrap();

    // 1. Two cards in a fresh address book, then a first sync.
    create_book(root, BOOK);
    put(root, BOOK, "card-1", &card("card-1", "Jane Doe"));
    put(root, BOOK, "card-2", &card("card-2", "John Doe"));

    neverest(&["init", "-a", ACCOUNT], &config, &state, 0);

    // A dry run over a store that has never synced must name the cards it
    // would fetch. A DAV kind resolves its link id only at `Full`, so
    // upgrading the probe would download every body to print a plan and leave
    // the placements looking complete, which reported an empty patch and
    // "already in sync" for an account holding nothing.
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
    assert!(
        items.contains(r#""link_id":"card-1""#) && items.contains(r#""link_id":"card-2""#),
        "both cards landed, keyed by their UID; store held:\n{items}",
    );
    assert!(
        items.contains("Jane Doe"),
        "the summary carries the display name; store held:\n{items}",
    );

    // 2. An in-place edit on the server: the ETag moves, so the sync must
    //    re-fetch the body rather than trust what it cached.
    put(root, BOOK, "card-1", &card("card-1", "Jane Rewritten"));

    // The re-fetch is reportable. A content change drops the stale object
    // while the hub keeps the level the item had reached, so a plan keyed on
    // the level would call an item whose body is about to be re-fetched done.
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
        items.contains("Jane Rewritten"),
        "the edited body reached the store; store held:\n{items}",
    );

    // 3. A server-side delete is retained, never lost.
    delete(BOOK, "card-2");
    sync(&config, &state, ACCOUNT, BOOK, 0);

    let live = store_items(&state, ACCOUNT, BOOK);
    assert!(
        !live.contains(r#""link_id":"card-2""#),
        "the deleted card left the live listing; store held:\n{live}",
    );
    let retained = pimdir(
        &state,
        ACCOUNT,
        &["item", "list", &collection(BOOK), "--retained", "--json"],
    );
    assert!(
        retained.contains(r#""link_id":"card-2""#),
        "the deleted card is retained, not lost; retained listing:\n{retained}",
    );

    // 4. Nothing changed since: a retained row must be invisible to the merge,
    //    not re-derived (which would re-upload it, forever).
    let report = sync(&config, &state, ACCOUNT, BOOK, 0);
    assert!(
        !report.contains("\"card-2\""),
        "a quiescent run touches nothing; report was:\n{report}",
    );
}

/// Proves the offline replica's happy conflict path end to end: a frontend
/// queues a note change while the server changes the phone number, and one
/// run merges the two and carries the merged body back to the server, with
/// nothing left waiting for a person.
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

    // The frontend's half: an edit staged into the queue against the body the
    // store just agreed on, which is the base the merge will read.
    queue_update(
        &state,
        REPLICA,
        MERGE_BOOK,
        "card-1",
        &noted_card("card-1", "+1", "new"),
    );
    // And the server's half, in a field the frontend left alone.
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

/// Proves the residual case parks rather than overwrites: the frontend and
/// the server both set the phone number, the run ends conflicted without
/// touching the server's body, a rerun changes nothing, and only a person's
/// `--prefer-local` converges it.
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

    // A run that leaves a decision waiting is neither a success nor a failure.
    let report = sync(&config, &state, REPLICA, COLLIDE_BOOK, 2);
    assert!(
        report.contains(r#""outstandingConflicts":1"#),
        "the divergence is parked and counted; report was:\n{report}",
    );
    assert!(
        report.contains(r#""id":"card-1.vcf""#),
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

    // A parked divergence waits for a person, however many runs go by.
    let report = sync(&config, &state, REPLICA, COLLIDE_BOOK, 2);
    assert!(
        !report.contains(r#""kind":"update""#),
        "a rerun pushes neither body over the other; report was:\n{report}",
    );
    assert!(
        get(COLLIDE_BOOK, "card-1").contains("TEL:+3"),
        "the server's body survives a rerun",
    );

    // The decision, then the run that carries it.
    let id = conflict_id(&listed, "card-1.vcf").to_string();
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

/// Proves an unreadable body is reported for what it is rather than counted
/// as a disagreement nobody had: the item parks, the run names it unmergeable,
/// and it never reads as a same-field collision.
///
/// NOTE: the unreadable body is the queued one rather than the server's.
/// Radicale validates on PUT and answers 400 to every malformed card, so no
/// body a parser refuses can be planted through its HTTP API; the merge's
/// `unparsed` arm names whichever of the three sides failed, so which one it
/// is does not change what is under test.
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
        2,
    );

    assert!(
        report.contains(r#""outstandingConflicts":1"#) && report.contains(r#""id":"card-1.vcf""#),
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

/// The address book as the store keys it: pimdir groups a collection under the
/// id of the source that syncs it, which for the direct-backend sugar is the
/// protocol name.
fn collection(book: &str) -> String {
    format!("carddav/{book}")
}

/// Stages an edit of one card into the store's action queue, the way a
/// frontend does it: the body lands in the blob tree first, then the action
/// pinning it is appended, so a collector never runs between the two.
///
/// This is the half of a single-endpoint conflict no server can produce, and
/// io-pimdir is neverest's own dependency, so the test writes it directly
/// rather than pretending a second endpoint is a frontend.
fn queue_update(state: &Path, account: &str, book: &str, link_id: &str, body: &str) {
    let dir = store_dir(state, account);
    let collection = collection(book);

    let reader = PimdirReader::open(&dir).expect("open the store to read");
    let seq = reader
        .seq_for_link(&collection, link_id)
        .expect("resolve the item's public id")
        .unwrap_or_else(|| panic!("no item {link_id} in {collection}"));
    let blobs = reader.blobs();
    drop(reader);

    let hash = blobs.hash(body.as_bytes());
    let mut writer = blobs.writer().expect("open a blob writer");
    writer.write_all(body.as_bytes()).expect("write the body");
    let size = writer.commit(&hash).expect("commit the body");

    let mut producer = PimdirProducer::open(&dir, "carddav-test").expect("open the store to stage");
    producer
        .enqueue(
            &collection,
            &PimdirAction::Update {
                seq,
                object: hash,
                meta: None,
            },
            Some(size),
            &Utc::now().to_rfc3339(),
        )
        .expect("stage the edit");
}

/// The id `conflict list --json` gave the divergence of `handle`, which is
/// what `conflict resolve` addresses it by.
fn conflict_id(listing: &str, handle: &str) -> i64 {
    let listed: serde_json::Value = serde_json::from_str(listing).expect("conflict listing");

    listed["conflicts"]
        .as_array()
        .expect("a conflict array")
        .iter()
        .find(|conflict| conflict["handle"] == handle)
        .unwrap_or_else(|| panic!("no conflict for {handle} in:\n{listing}"))["id"]
        .as_i64()
        .expect("a conflict id")
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

/// The store's live items for an address book, as the `pimdir` CLI renders
/// them.
fn store_items(state: &Path, account: &str, book: &str) -> String {
    pimdir(
        state,
        account,
        &["item", "list", &collection(book), "--json"],
    )
}

/// Where the account's pimdir store lives under the run's state directory.
fn store_dir(state: &Path, account: &str) -> std::path::PathBuf {
    state.join("neverest").join(account)
}

/// Runs the `pimdir` operator CLI against the account's store. It is the
/// observation tool for everything the sync did not print, retention above
/// all, and it comes from the io-pimdir crate rather than from neverest.
fn pimdir(state: &Path, account: &str, args: &[&str]) -> String {
    let store = store_dir(state, account);
    let output = Command::new("pimdir")
        .args(["--store", &store.to_string_lossy()])
        .args(args)
        .output()
        .expect("spawn pimdir (cargo install --path ../io-pimdir --features cli)");

    assert!(
        output.status.success(),
        "`pimdir {}` failed:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8_lossy(&output.stdout).into_owned()
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
