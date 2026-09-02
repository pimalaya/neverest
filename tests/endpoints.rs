//! End-to-end test of an account with TWO CardDAV endpoints, against a local
//! Radicale (`tests/radicale.sh`) holding two principals. Ignored by default.
//!
//! The one-endpoint run in tests/carddav.rs reconciles a store against a
//! server. This one reconciles two servers with each other through the store,
//! which is the mirror and migration case, and the three things that only
//! happen there:
//!
//!   1. A card both endpoints changed differently. Each of them agrees with its
//!      own server, so only the pair disagrees, and the run must merge what
//!      nobody disagreed about and park the rest rather than let one endpoint's
//!      body land on the other.
//!   2. Two servers that already hold the same card before the first sync. One
//!      identity is one item, whichever endpoint the store reads first: minting
//!      a second key for the second holder leaves two items neither server will
//!      take.
//!   3. Two servers holding one identity under two different bodies before the
//!      first sync. There is no body they both came from, so no merge can
//!      settle it, and the run must park rather than let the source's body
//!      replace what the target already held.
//!   4. The same divergence as the first, under an account declaring the
//!      source authoritative. There is nothing to park then: `one-way = true`
//!      is the answer to the question the other three ask a person.
//!
//! An account naming a source and a target keys the store under the source's
//! name, so an address book reads as `a/<name>` here rather than as the
//! `carddav/<name>` the direct-backend sugar of tests/carddav.rs uses.
//!
//! Each test owns an address book of its own on both principals and narrows the
//! run to it, so the two run side by side and neither meets what the other
//! seeded.
//!
//! Start the server and run with:
//! ```sh
//! ./tests/radicale.sh
//! cargo test --features dav --test endpoints -- --ignored
//! ```

use std::{fs, path::Path, process::Command};

const DAV: &str = "http://127.0.0.1:5232";
/// The address book the divergence test owns on both principals.
const MERGE_BOOK: &str = "merge";
/// The address book the binding test owns on both principals.
const BIND_BOOK: &str = "bind";
/// The address book the ancestor-less divergence test owns on both principals.
const SEED_BOOK: &str = "seed";
/// The address book the one-way test owns on both principals.
const AUTHORITY_BOOK: &str = "authority";
/// The source endpoint's principal, whose password is its own name.
const SOURCE: &str = "test";
/// The target endpoint's principal.
const TARGET: &str = "test2";

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

#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_card_changed_on_both_endpoints_merges_or_parks_and_never_overwrites() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account()).unwrap();

    create_book(root, SOURCE, MERGE_BOOK);
    create_book(root, TARGET, MERGE_BOOK);

    // Seeded on one endpoint only, so the first sync binds both cards on both:
    // seeding both is the create collision the other test starts from.
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

    // card-1 is edited on each endpoint in a field the other left alone, and
    // card-2 in the same field on both. The first is not a disagreement and the
    // three-way merge settles it; the second is, and no merge can.
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

    // A run leaving the two endpoints divergent is not a success: it ends
    // conflicted and names the item it parked.
    let report = sync(&config, &state, MERGE_BOOK, 2);
    assert!(
        report.contains(r#""outstandingConflicts":1"#),
        "the divergence is parked and counted, not resolved; report was:\n{report}",
    );
    assert!(
        report.contains(r#""id":"card-2.vcf""#),
        "the parked item is named; report was:\n{report}",
    );

    // The disjoint edits merged, so both endpoints carry both of them.
    for side in [SOURCE, TARGET] {
        let merged = get(side, MERGE_BOOK, "card-1");
        assert!(
            merged.contains("TEL:+2") && merged.contains("NOTE:new"),
            "{side} holds the merged card-1; it held:\n{merged}",
        );
    }

    // The same-field collision parked, so each endpoint still holds its own
    // edit and neither was overwritten by the other.
    assert!(
        get(SOURCE, MERGE_BOOK, "card-2").contains("TEL:+2"),
        "the source keeps its own card-2 edit",
    );
    assert!(
        get(TARGET, MERGE_BOOK, "card-2").contains("TEL:+3"),
        "the target keeps its own card-2 edit",
    );

    // Rerunning settles nothing on its own and overwrites nothing either: a
    // parked divergence waits for a person, however many runs go by.
    let report = sync(&config, &state, MERGE_BOOK, 2);
    assert!(
        !report.contains(r#""kind":"update""#),
        "a rerun pushes neither body over the other; report was:\n{report}",
    );
    assert!(
        get(SOURCE, MERGE_BOOK, "card-2").contains("TEL:+2")
            && get(TARGET, MERGE_BOOK, "card-2").contains("TEL:+3"),
        "both edits survive a rerun",
    );

    // And it is a decision the conflict commands can find.
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
/// The source is authoritative, so the difference the two-way account leaves
/// for a person resolves in its favour: the target's own edit is overwritten,
/// nothing is counted or listed as a conflict, the run exits 0, and the next
/// one has nothing left to carry.
#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_one_way_account_overwrites_the_target_instead_of_parking_the_divergence() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    // NOTE: the account is one TOML table and the flag belongs to it wherever
    // it sits, so appending keeps one account helper for both directions.
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

    // The same field set two ways, which is the divergence the two-way account
    // parks: each endpoint agrees with its own server and only the pair
    // disagrees.
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

    // Declaring an authority is what removes the conflict rather than
    // resolving it: the run has nothing to ask, so it ends on 0.
    let report = sync(&config, &state, AUTHORITY_BOOK, 0);
    assert!(
        report.contains(r#""outstandingConflicts":0"#),
        "an authoritative source leaves nothing waiting; report was:\n{report}",
    );
    assert!(
        !report.contains(r#""conflicts""#),
        "and names nothing parked; report was:\n{report}",
    );

    // The overwrite lands in this run: the deciding side's body is hydrated
    // before the pushing passes, so the target takes it without waiting for
    // the run after.
    assert!(
        get(TARGET, AUTHORITY_BOOK, "card-1").contains("TEL:+2"),
        "the target takes the source's body, its own edit discarded",
    );
    assert!(
        get(SOURCE, AUTHORITY_BOOK, "card-1").contains("TEL:+2"),
        "and the source is never written back to, which is what makes it \
         authoritative",
    );

    // Discarded means settled: a rerun writes nothing and asks nothing, rather
    // than carrying the same overwrite again every run.
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

#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn two_endpoints_already_holding_one_card_bind_it_to_a_single_item() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account()).unwrap();

    create_book(root, SOURCE, BIND_BOOK);
    create_book(root, TARGET, BIND_BOOK);

    // The same card on both servers before the store has read either of them,
    // which is what a mirror or a migration starts from.
    put(root, SOURCE, BIND_BOOK, "card-1", &card("card-1", "+1"));
    put(root, TARGET, BIND_BOOK, "card-1", &card("card-1", "+1"));

    neverest(&["init", "-a", "div"], &config, &state, 0);
    let report = sync(&config, &state, BIND_BOOK, 0);
    assert!(
        !report.contains("dup:"),
        "the second holder is bound, not minted a key of its own; report was:\n{report}",
    );
    assert!(
        !report.contains(r#""refused""#) && !report.contains(r#""rejected""#),
        "neither endpoint is asked to take a card it already holds; report was:\n{report}",
    );

    let items = store_items(&state, BIND_BOOK);
    assert!(
        items.contains(r#""link_id":"card-1""#) && !items.contains("dup:"),
        "one identity is one item; store held:\n{items}",
    );
    assert_eq!(
        items.matches(r#""link_id""#).count(),
        1,
        "and only one; store held:\n{items}",
    );

    // Bound on both endpoints means the run has nothing left to do.
    let report = sync(&config, &state, BIND_BOOK, 0);
    assert!(
        report.contains(r#""patch":[]"#),
        "a bound pair is quiescent; report was:\n{report}",
    );
}

#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn two_endpoints_holding_one_card_under_two_bodies_park_it_and_never_overwrite() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account()).unwrap();

    create_book(root, SOURCE, SEED_BOOK);
    create_book(root, TARGET, SEED_BOOK);

    // One identity, two bodies, on two servers the store has never read: there
    // is no body they both came from, so the three-way merge has no base and
    // nothing but a person can pick a winner.
    for uid in ["card-1", "card-2"] {
        put(root, SOURCE, SEED_BOOK, uid, &card(uid, "+1"));
        put(root, TARGET, SEED_BOOK, uid, &card(uid, "+9"));
    }

    neverest(&["init", "-a", "div"], &config, &state, 0);

    // The first run must leave both servers exactly as it found them, and say
    // so: a divergence nobody chose to lose is not an update.
    let report = sync(&config, &state, SEED_BOOK, 2);
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

    // Rerunning settles nothing and overwrites nothing either, and names the
    // divergence once rather than once per run.
    let report = sync(&config, &state, SEED_BOOK, 2);
    assert!(
        !report.contains(r#""kind":"update""#) && !report.contains(r#""conflicts""#),
        "a rerun pushes nothing and re-announces nothing; report was:\n{report}",
    );
    assert!(
        get(TARGET, SEED_BOOK, "card-1").contains("TEL:+9"),
        "the target's body survives a rerun",
    );

    // Both are decisions the conflict commands can find and settle, either way
    // round: the local body is the one the store holds, which the source
    // contributed, and the remote body is the target's own.
    let listed = neverest(
        &["conflict", "list", "-a", "div", "--json"],
        &config,
        &state,
        0,
    );
    let local = conflict_id(&listed, "card-1.vcf").to_string();
    let remote = conflict_id(&listed, "card-2.vcf").to_string();

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

    // One run then carries each decision to both endpoints, and neither of the
    // two bodies was lost on the way to it.
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

/// The id `conflict list --json` gave the divergence of `handle`, which is what
/// `conflict resolve` addresses it by.
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

/// The account every test runs: one source and one target, each a principal of
/// the same Radicale, keeping a local copy of what crosses.
fn account() -> String {
    format!(
        "[accounts.div]\n\
         retain = true\n\
         sources.a.carddav.server = \"{DAV}/\"\n\
         sources.a.carddav.auth.basic.username = \"{SOURCE}\"\n\
         sources.a.carddav.auth.basic.password.raw = \"{SOURCE}\"\n\
         targets.b.carddav.server = \"{DAV}/\"\n\
         targets.b.carddav.auth.basic.username = \"{TARGET}\"\n\
         targets.b.carddav.auth.basic.password.raw = \"{TARGET}\"\n",
    )
}

/// Recreates a principal's address book, so a run starts from an empty one
/// whatever the previous run left behind.
///
/// Radicale does not create a collection on a member write (it answers the PUT
/// with a 409), and the container keeps its storage between runs, so the book is
/// deleted and made again with an explicit extended `MKCOL`.
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

/// The store's live items for an address book, as the `pimdir` CLI renders
/// them.
fn store_items(state: &Path, book: &str) -> String {
    let store = state.join("neverest").join("div");
    let output = Command::new("pimdir")
        .args(["--store", &store.to_string_lossy()])
        .args(["item", "list", &format!("a/{book}"), "--json"])
        .output()
        .expect("spawn pimdir (cargo install --path ../io-pimdir --features cli)");

    assert!(
        output.status.success(),
        "`pimdir item list a/{book}` failed:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Runs neverest and checks it ended on `code`, 2 being a run that reconciled
/// and left a decision waiting.
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
