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
//! Start the server and run with:
//! ```sh
//! ./tests/radicale.sh
//! cargo test --features carddav --test carddav -- --ignored
//! ```

use std::{fs, path::Path, process::Command};

const DAV: &str = "http://127.0.0.1:5232";
const USER: &str = "test";
const PASS: &str = "test";
const BOOK: &str = "contacts";

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

#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_carddav_side_syncs_edits_and_retains_a_server_delete() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();

    fs::write(
        &config,
        format!(
            "[accounts.contacts]\n\
             left.carddav.server = \"{DAV}/\"\n\
             left.carddav.auth.basic.username = \"{USER}\"\n\
             left.carddav.auth.basic.password.raw = \"{PASS}\"\n",
        ),
    )
    .unwrap();

    // 1. Two cards in a fresh address book, then a first sync.
    create_book(root);
    put(root, "card-1", &card("card-1", "Jane Doe"));
    put(root, "card-2", &card("card-2", "John Doe"));

    neverest(&["init", "-a", "contacts"], &config, &state);
    neverest(&["sync", "-a", "contacts"], &config, &state);

    let items = store_items(&state, BOOK);
    assert!(
        items.contains("uid:card-1") && items.contains("uid:card-2"),
        "both cards landed, keyed by their UID; store held:\n{items}",
    );
    assert!(
        items.contains("Jane Doe"),
        "the summary carries the display name; store held:\n{items}",
    );

    // 2. An in-place edit on the server: the ETag moves, so the sync must
    //    re-fetch the body rather than trust what it cached.
    put(root, "card-1", &card("card-1", "Jane Rewritten"));
    neverest(&["sync", "-a", "contacts"], &config, &state);

    let items = store_items(&state, BOOK);
    assert!(
        items.contains("Jane Rewritten"),
        "the edited body reached the store; store held:\n{items}",
    );

    // 3. A server-side delete is retained, never lost.
    delete(root, "card-2");
    neverest(&["sync", "-a", "contacts"], &config, &state);

    let live = store_items(&state, BOOK);
    assert!(
        !live.contains("uid:card-2"),
        "the deleted card left the live listing; store held:\n{live}",
    );
    let retained = pimdir(&state, &["item", "list", BOOK, "--retained", "--json"]);
    assert!(
        retained.contains("uid:card-2"),
        "the deleted card is retained, not lost; retained listing:\n{retained}",
    );

    // 4. Nothing changed since: a retained row must be invisible to the merge,
    //    not re-derived (which would re-upload it, forever).
    let report = neverest(&["sync", "-a", "contacts", "--json"], &config, &state);
    assert!(
        !report.contains("\"card-2\""),
        "a quiescent run touches nothing; report was:\n{report}",
    );
}

/// Recreates the address book on the server, so a run starts from an empty
/// one whatever the previous run left behind.
///
/// Radicale does not create a collection on a member write (it answers the
/// PUT with a 409), and the container keeps its storage between runs, so the
/// book is deleted and made again with an explicit extended `MKCOL`.
fn create_book(root: &Path) {
    let output = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-X", "DELETE"])
        .args(["-u", &format!("{USER}:{PASS}")])
        .arg(format!("{DAV}/{USER}/{BOOK}/"))
        .output()
        .expect("spawn curl delete book");
    assert!(output.status.success(), "DELETE of the address book failed");

    let body = root.join("mkcol.xml");
    fs::write(
        &body,
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <D:mkcol xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:carddav\">\
         <D:set><D:prop>\
         <D:resourcetype><D:collection/><C:addressbook/></D:resourcetype>\
         <D:displayname>contacts</D:displayname>\
         </D:prop></D:set></D:mkcol>",
    )
    .unwrap();

    let output = Command::new("curl")
        .args(["-fsS", "-X", "MKCOL", "-u", &format!("{USER}:{PASS}")])
        .args(["-H", "Content-Type: application/xml; charset=utf-8"])
        .arg("--data-binary")
        .arg(format!("@{}", body.display()))
        .arg(format!("{DAV}/{USER}/{BOOK}/"))
        .output()
        .expect("spawn curl mkcol");

    assert!(
        output.status.success(),
        "MKCOL {BOOK} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Writes a card to the server.
fn put(root: &Path, id: &str, body: &str) {
    let file = root.join(format!("{id}.vcf"));
    fs::write(&file, body).unwrap();

    let output = Command::new("curl")
        .args(["-fsS", "-X", "PUT", "-u", &format!("{USER}:{PASS}")])
        .args(["-H", "Content-Type: text/vcard; charset=utf-8"])
        .arg("--data-binary")
        .arg(format!("@{}", file.display()))
        .arg(format!("{DAV}/{USER}/{BOOK}/{id}.vcf"))
        .output()
        .expect("spawn curl put");

    assert!(
        output.status.success(),
        "PUT {id} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Deletes a card on the server.
fn delete(root: &Path, id: &str) {
    let _ = root;
    let output = Command::new("curl")
        .args(["-fsS", "-X", "DELETE", "-u", &format!("{USER}:{PASS}")])
        .arg(format!("{DAV}/{USER}/{BOOK}/{id}.vcf"))
        .output()
        .expect("spawn curl delete");

    assert!(
        output.status.success(),
        "DELETE {id} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// The store's live items for a collection, as the `pimdir` CLI renders them.
fn store_items(state: &Path, collection: &str) -> String {
    pimdir(state, &["item", "list", collection, "--json"])
}

/// Runs the `pimdir` operator CLI against the account's store. It is the
/// observation tool for everything the sync did not print, retention above
/// all, and it comes from the io-pimdir crate rather than from neverest.
fn pimdir(state: &Path, args: &[&str]) -> String {
    let store = state.join("neverest").join("contacts");
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
