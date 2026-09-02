//! End-to-end test of one account holding several kinds at once, against a
//! local Stalwart (`tests/stalwart2.sh`, server A) and a local Radicale
//! (`tests/radicale.sh`). Ignored by default.
//!
//! Every other live test drives one kind. An account may declare mail and
//! contacts side by side over one store, and that is where the per-kind
//! dispatch is decided rather than assumed:
//!
//!   1. One run carries both. The report is account-wide, so a mailbox and an
//!      address book reconciled in the same breath both reach it.
//!   2. The collections stay keyed apart. A store keys a collection under the
//!      id of the source that syncs it, so a mailbox and an address book
//!      never meet however alike their names are.
//!   3. The dispatch is right. Contacts have mutable bodies, so a card that
//!      moved on both sides parks and waits for a person; mail bodies are
//!      immutable, so its axis is flags and deletes and a server-side change
//!      to either simply applies.
//!   4. The exit code answers the parked item and nothing else. A run that
//!      reconciled its mail and parked a card is neither a success nor a
//!      failure, whichever kind the parked item belongs to.
//!
//! The mail work is made on the server, flags and a delete being the only
//! axis mail has. The card divergence is made the way a frontend makes one,
//! by staging an edit through the store's queue while the server takes an
//! edit of its own: two sides moving away from a body they agreed on.
//!
//! The account owns a mailbox and an address book of its own and narrows the
//! run to both, so it meets nothing another test seeded.
//!
//! Start the servers and run with:
//! ```sh
//! ./tests/radicale.sh
//! ./tests/stalwart2.sh
//! cargo test --all-features --test multikind -- --ignored
//! ```

use std::{fs, io::Write, path::Path, process::Command};

use io_pimdir::{PimdirBlobs, PimdirProducer, PimdirReader, codec::PimdirAction};
use io_replica::collection::ReplicaCollectionId;

const IMAP_ROOT: &str = "imap://127.0.0.1:143";
const IMAP_USER: &str = "test@pimalaya.org";
const IMAP_PASS: &str = "P!malaya-test-2026";
const DAV: &str = "http://127.0.0.1:5232";
/// The Radicale principal, whose password is its own name.
const DAV_USER: &str = "test";
/// The account declaring both kinds, whose name also keys its store.
const ACCOUNT: &str = "multikind";
/// The mailbox this account owns on the IMAP server.
const MAILBOX: &str = "multikindmail";
/// The address book this account owns on the CardDAV server.
const BOOK: &str = "multikindcards";
/// The mailbox as the store keys it, under the id of the source syncing it.
const MAIL_COLLECTION: &str = "imap/multikindmail";
/// The address book as the store keys it, under its own source.
const CARD_COLLECTION: &str = "carddav/multikindcards";
/// The card this account diverges, addressed on the server by `<uid>.vcf`.
const CARD: &str = "multikind-card";
/// The marker of the message whose flags move on the server.
const KEPT: &str = "multikind-kept";
/// The marker of the message the server expunges.
const GONE: &str = "multikind-gone";

/// A card carrying one phone number, the field the two sides set differently.
fn card(tel: &str) -> String {
    format!(
        "BEGIN:VCARD\r\n\
         VERSION:4.0\r\n\
         UID:{CARD}\r\n\
         FN:Jane Doe\r\n\
         TEL:{tel}\r\n\
         END:VCARD\r\n",
    )
}

/// A message keyed by `marker`, which is also the link id the store binds it
/// under, neverest keying mail on the bare `Message-ID`.
fn message(marker: &str) -> Vec<u8> {
    format!(
        "Message-ID: <{marker}@pimalaya.org>\r\n\
         From: alice@pimalaya.org\r\n\
         To: bob@pimalaya.org\r\n\
         Subject: neverest multikind {marker}\r\n\
         Date: Tue, 25 Aug 2026 10:00:00 +0000\r\n\
         \r\n\
         {marker}\r\n",
    )
    .into_bytes()
}

#[test]
#[ignore = "requires Stalwart (./tests/stalwart2.sh) on :143, Radicale (./tests/radicale.sh) on :5232 and --ignored"]
fn one_account_syncing_mail_and_contacts_carries_both_and_dispatches_each_by_kind() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account()).unwrap();

    // 1. A mailbox holding two messages and an address book holding one card,
    //    both of this account's own, then a first run that agrees on all
    //    three: the base every later divergence is measured against.
    create_mailbox();
    append(root, KEPT);
    append(root, GONE);
    create_book(root);
    put(root, &card("+1"));

    neverest(&["init", "-a", ACCOUNT], &config, &state, 0);
    sync(&config, &state, 0);

    let store = state.join("neverest").join(ACCOUNT);
    assert_eq!(links(&store, MAIL_COLLECTION).len(), 2, "both messages");
    assert_eq!(links(&store, CARD_COLLECTION).len(), 1, "the card");

    // 2. Mail's own axis: a flag set on one message and the other expunged,
    //    both on the server. Neither is a body change, mail having none.
    imap("STORE 1 +Flags \\Flagged");
    imap("STORE 2 +Flags \\Deleted");
    imap("EXPUNGE");

    // 3. The contacts axis: the card edited on the server while the store
    //    holds an edit of its own, staged through the queue the way a
    //    frontend stages one. Neither side can be dropped for the other.
    edit_card_in_store(&store, &card("+2"));
    put(root, &card("+3"));

    // 4. One run, both kinds. It reconciled what it could and left one item
    //    waiting, which is exactly what exit code 2 says.
    let report = sync(&config, &state, 2);
    assert!(
        report.contains(r#""outstandingConflicts":1"#),
        "one item is waiting for a decision, no more; report was:\n{report}",
    );
    assert!(
        report.contains(&format!(
            r#"{{"side":"carddav","collection":"{BOOK}","id":"{CARD}.vcf"}}"#
        )),
        "the contacts side is named as the one that parked; report was:\n{report}",
    );
    assert!(
        report.contains(&format!(
            r#""kind":"add-flags","side":"imap","collection":"{MAILBOX}""#
        )),
        "and the mail side as one that applied, in the same report; report was:\n{report}",
    );
    assert!(
        report.contains(&format!(
            r#""kind":"delete","side":"imap","collection":"{MAILBOX}""#
        )),
        "along with the removal the server made; report was:\n{report}",
    );

    // 5. The card parked. Mail never reaches this path: its bodies are
    //    immutable, so a divergence of content cannot arise there at all.
    let listed = neverest(
        &["conflict", "list", "-a", ACCOUNT, "--json"],
        &config,
        &state,
        0,
    );
    let listed: serde_json::Value = serde_json::from_str(&listed).expect("conflict listing");
    let conflicts = listed["conflicts"].as_array().expect("a conflict array");
    assert_eq!(conflicts.len(), 1, "one parked item: {listed}");
    assert_eq!(
        conflicts[0]["collection"], CARD_COLLECTION,
        "the parked item is the card, not a message: {listed}",
    );
    assert_eq!(
        conflicts[0]["handle"],
        format!("{CARD}.vcf"),
        "named by its handle on the CardDAV source: {listed}",
    );

    // Parked means untouched: the server keeps its own body and the run
    // pushed neither side over the other.
    assert!(
        get(CARD).contains("TEL:+3"),
        "the CardDAV server keeps its own edit",
    );

    // 6. The mail dispatch, on the axis mail does have. The flag change
    //    applied, and the expunge retained the item rather than losing it.
    let reader = PimdirReader::open(&store).expect("open the store");
    let kept = reader
        .list_items(MAIL_COLLECTION, None, 10)
        .expect("list the mailbox")
        .into_iter()
        .find(|item| item.link_id.0.contains(KEPT))
        .expect("the kept message is still live");
    assert!(
        kept.flags.contains("\\Flagged"),
        "the flag the server set applied, no decision needed; flags were {:?}",
        kept.flags,
    );

    let live = links(&store, MAIL_COLLECTION);
    assert_eq!(live.len(), 1, "the expunged message left the live listing");
    let retained = reader
        .list_retained(&ReplicaCollectionId(MAIL_COLLECTION.into()), None, 10)
        .expect("list the retained mail");
    assert!(
        retained.iter().any(|item| item.link_id.0.contains(GONE)),
        "and is retained rather than lost",
    );

    // 7. The two collections never met, however alike a mailbox and an
    //    address book are named: each holds its own items and nothing else.
    let collections: Vec<String> = reader
        .list_collections()
        .expect("list the store collections")
        .into_iter()
        .map(|collection| collection.id)
        .collect();
    assert!(
        collections.iter().any(|id| id == MAIL_COLLECTION)
            && collections.iter().any(|id| id == CARD_COLLECTION),
        "both kinds are keyed under their own source; store held:\n{collections:?}",
    );
    assert!(
        !links(&store, MAIL_COLLECTION)
            .iter()
            .any(|link| link.contains(CARD)),
        "no card reached the mailbox",
    );
    assert_eq!(
        links(&store, CARD_COLLECTION),
        vec![String::from(CARD)],
        "and no message reached the address book",
    );
}

/// The account: one IMAP source and one CardDAV source over one store, each
/// written as the direct-backend sugar, so each keys its own collections.
fn account() -> String {
    format!(
        "[accounts.{ACCOUNT}]\n\
         imap.server = \"{IMAP_ROOT}\"\n\
         imap.starttls = false\n\
         imap.sasl.plain.username = \"{IMAP_USER}\"\n\
         imap.sasl.plain.password.raw = \"{IMAP_PASS}\"\n\
         carddav.server = \"{DAV}/\"\n\
         carddav.auth.basic.username = \"{DAV_USER}\"\n\
         carddav.auth.basic.password.raw = \"{DAV_USER}\"\n",
    )
}

/// Syncs the account, narrowed to the mailbox and the address book it owns.
fn sync(config: &Path, state: &Path, code: i32) -> String {
    neverest(
        &["sync", "-a", ACCOUNT, "-m", MAILBOX, "-m", BOOK, "--json"],
        config,
        state,
        code,
    )
}

/// Stages a body for the card through the store's queue, exactly as a
/// frontend stages an edit: the blob durably in the tree first, then the
/// queue row naming it, which the next run drains before it reconciles.
fn edit_card_in_store(store: &Path, body: &str) {
    let reader = PimdirReader::open(store).expect("open the store");
    let seq = reader
        .list_items(CARD_COLLECTION, None, 10)
        .expect("list the address book")
        .first()
        .expect("the card is in the store")
        .seq;

    let mut producer = PimdirProducer::open(store, "neverest-tests").expect("open producer");
    let hash = producer.hash(body.as_bytes());
    let blobs = PimdirBlobs::open(store, producer.hash_algo());
    let mut writer = blobs.writer().expect("blob writer");
    writer.write_all(body.as_bytes()).unwrap();
    let size = writer.commit(&hash).expect("commit body");

    producer
        .enqueue(
            CARD_COLLECTION,
            &PimdirAction::Update {
                seq,
                object: hash,
                meta: None,
            },
            Some(size),
            "2026-09-02T10:00:00Z",
        )
        .expect("enqueue the edit");
}

/// The live link ids of one collection, which is what says an item is where
/// it belongs and nowhere else.
fn links(store: &Path, collection: &str) -> Vec<String> {
    PimdirReader::open(store)
        .expect("open the store")
        .list_items(collection, None, 100)
        .expect("list the collection")
        .into_iter()
        .map(|item| item.link_id.0)
        .collect()
}

/// Recreates the account's mailbox, so a run starts from an empty one
/// whatever the previous run left behind.
///
/// The server keeps its storage between runs, and the `DELETE` of a mailbox
/// that is not there is an error rather than a no-op, so only the `CREATE` is
/// checked.
fn create_mailbox() {
    let _ = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "--url", IMAP_ROOT])
        .args(["--user", &format!("{IMAP_USER}:{IMAP_PASS}")])
        .args(["-X", &format!("DELETE {MAILBOX}")])
        .output()
        .expect("spawn curl delete mailbox");

    let output = Command::new("curl")
        .args(["-fsS", "-o", "/dev/null", "--url", IMAP_ROOT])
        .args(["--user", &format!("{IMAP_USER}:{IMAP_PASS}")])
        .args(["-X", &format!("CREATE {MAILBOX}")])
        .output()
        .expect("spawn curl create mailbox");

    assert!(
        output.status.success(),
        "CREATE {MAILBOX} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Appends one message to the account's mailbox.
fn append(root: &Path, marker: &str) {
    let file = root.join(format!("{marker}.eml"));
    fs::write(&file, message(marker)).unwrap();

    let output = Command::new("curl")
        .args(["-fsS", "-T"])
        .arg(&file)
        .arg(format!("{IMAP_ROOT}/{MAILBOX}"))
        .args(["--user", &format!("{IMAP_USER}:{IMAP_PASS}")])
        .output()
        .expect("spawn curl append");

    assert!(
        output.status.success(),
        "APPEND {marker} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Runs one IMAP command against the account's mailbox, which is how the
/// server-side flag change and expunge are made.
fn imap(command: &str) {
    let output = Command::new("curl")
        .args(["-fsS", "-o", "/dev/null"])
        .arg(format!("{IMAP_ROOT}/{MAILBOX}"))
        .args(["--user", &format!("{IMAP_USER}:{IMAP_PASS}")])
        .args(["-X", command])
        .output()
        .expect("spawn curl imap command");

    assert!(
        output.status.success(),
        "`{command}` failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Recreates the account's address book, so a run starts from an empty one
/// whatever the previous run left behind.
///
/// Radicale does not create a collection on a member write (it answers the
/// PUT with a 409), and the container keeps its storage between runs, so the
/// book is deleted and made again with an explicit extended `MKCOL`.
fn create_book(root: &Path) {
    let output = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-X", "DELETE"])
        .args(["-u", &format!("{DAV_USER}:{DAV_USER}")])
        .arg(format!("{DAV}/{DAV_USER}/{BOOK}/"))
        .output()
        .expect("spawn curl delete book");
    assert!(output.status.success(), "DELETE of the address book failed");

    let body = root.join("mkcol.xml");
    fs::write(
        &body,
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <D:mkcol xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:carddav\">\
             <D:set><D:prop>\
             <D:resourcetype><D:collection/><C:addressbook/></D:resourcetype>\
             <D:displayname>{BOOK}</D:displayname>\
             </D:prop></D:set></D:mkcol>",
        ),
    )
    .unwrap();

    let output = Command::new("curl")
        .args([
            "-fsS",
            "-X",
            "MKCOL",
            "-u",
            &format!("{DAV_USER}:{DAV_USER}"),
        ])
        .args(["-H", "Content-Type: application/xml; charset=utf-8"])
        .arg("--data-binary")
        .arg(format!("@{}", body.display()))
        .arg(format!("{DAV}/{DAV_USER}/{BOOK}/"))
        .output()
        .expect("spawn curl mkcol");

    assert!(
        output.status.success(),
        "MKCOL {BOOK} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Writes the card to the account's address book.
fn put(root: &Path, body: &str) {
    let file = root.join("card.vcf");
    fs::write(&file, body).unwrap();

    let output = Command::new("curl")
        .args(["-fsS", "-X", "PUT", "-u", &format!("{DAV_USER}:{DAV_USER}")])
        .args(["-H", "Content-Type: text/vcard; charset=utf-8"])
        .arg("--data-binary")
        .arg(format!("@{}", file.display()))
        .arg(format!("{DAV}/{DAV_USER}/{BOOK}/{CARD}.vcf"))
        .output()
        .expect("spawn curl put");

    assert!(
        output.status.success(),
        "PUT {CARD} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Reads the card back from the account's address book.
fn get(id: &str) -> String {
    let output = Command::new("curl")
        .args(["-fsS", "-u", &format!("{DAV_USER}:{DAV_USER}")])
        .arg(format!("{DAV}/{DAV_USER}/{BOOK}/{id}.vcf"))
        .output()
        .expect("spawn curl get");

    assert!(
        output.status.success(),
        "GET {id} failed: {}",
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
