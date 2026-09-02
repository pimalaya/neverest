//! End-to-end test of the external merger contract, against a local Radicale
//! (`tests/radicale.sh`) holding two principals. Ignored by default.
//!
//! `conflict.merger` hands a parked divergence to a program of the person's
//! choosing, and everything about that hand-off happens outside the process:
//! the argv order, the shell quoting of the exported paths, and the two ways
//! a merger says no. The in-process tests drive a `Merger` value directly, so
//! none of it had ever crossed a real fork against a real divergence:
//!
//!   1. The four paths arrive base, local, remote, output, under a directory
//!      whose name holds a space, and the body the merger writes is the one
//!      that lands and reaches both servers.
//!   2. A merger exiting non-zero settles nothing, having written its output
//!      or not: the divergence stays parked and both bodies stay untouched.
//!   3. A merger exiting zero without writing settles nothing either, which
//!      is what a bare quit in an editor looks like.
//!   4. A merger naming `{output}` is substituted rather than appended, so a
//!      tool whose output is a flag is not handed four trailing paths.
//!
//! The divergence is a real one, driven over two CardDAV endpoints the way
//! tests/endpoints.rs does: one card, edited in the same field on each
//! principal over a body they both came from, which is the shape that has a
//! base to export and no winner a run can pick.
//!
//! Each test owns an address book of its own on both principals and narrows
//! the run to it, so the four run side by side and none meets what another
//! seeded.
//!
//! Start the server and run with:
//! ```sh
//! ./tests/radicale.sh
//! cargo test --features dav --test merger -- --ignored
//! ```

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const DAV: &str = "http://127.0.0.1:5232";
/// The account every test declares, whose name also keys its store.
const ACCOUNT: &str = "merger";
/// The source endpoint's principal, whose password is its own name.
const SOURCE: &str = "test";
/// The target endpoint's principal.
const TARGET: &str = "test2";
/// The one card every test diverges, addressed on the server by `<uid>.vcf`.
const CARD: &str = "merger-card";
/// The address book the argv-order test owns on both principals.
const ORDER_BOOK: &str = "mergerorder";
/// The address book the non-zero-exit test owns on both principals.
const REFUSE_BOOK: &str = "mergerrefuse";
/// The address book the bare-quit test owns on both principals.
const QUIT_BOOK: &str = "mergerquit";
/// The address book the placeholder test owns on both principals.
const FLAG_BOOK: &str = "mergerflag";

/// A card carrying one phone number, the field the two endpoints set
/// differently and the merger settles.
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

/// The same card as a `printf` format a merger script writes with, the body
/// it settles on being one it composed rather than one of the three it read.
fn printf_card(tel: &str) -> String {
    format!(
        "printf 'BEGIN:VCARD\\r\\nVERSION:4.0\\r\\nUID:{CARD}\\r\\n\
         FN:Jane Doe\\r\\nTEL:{tel}\\r\\nEND:VCARD\\r\\n'",
    )
}

/// The shell prelude a logging merger opens with: the phone number of the
/// card at a path, which is what tells the three exported bodies apart.
const TEL: &str = "#!/bin/sh\ntel() { sed -n 's/^TEL://p' \"$1\" | tr -d '\\r'; }\n";

#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_merger_is_handed_base_local_remote_output_and_its_body_is_the_one_that_lands() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let log = root.join("argv.log");
    let script = root.join("order.sh");

    // The three bodies are read by their content rather than by their name, so
    // the log proves the order and not just the arity: base is the body the
    // first sync agreed on, local the one the store took from the source, and
    // remote the target's own.
    fs::write(
        &script,
        format!(
            "{TEL}\
             {{\n\
             echo \"argc=$#\"\n\
             echo \"base=$(basename \"$1\"):$(tel \"$1\")\"\n\
             echo \"local=$(basename \"$2\"):$(tel \"$2\")\"\n\
             echo \"remote=$(basename \"$3\"):$(tel \"$3\")\"\n\
             echo \"output=$(basename \"$4\"):$(wc -c < \"$4\" | tr -d ' ')\"\n\
             echo \"dir=$(dirname \"$4\")\"\n\
             }} > '{log}'\n\
             {write} > \"$4\"\n",
            log = log.display(),
            write = printf_card("+7"),
        ),
    )
    .unwrap();

    let parked = park(root, ORDER_BOOK, &format!("\"sh {}\"", script.display()));
    let settled = resolve(&parked);

    let argv = fs::read_to_string(&log).expect("the merger ran and logged its argv");
    assert!(
        argv.contains("argc=4"),
        "the four paths are appended and nothing else; argv was:\n{argv}",
    );
    assert!(
        argv.contains("base=base.vcf:+1"),
        "the first path is the base the last sync agreed on; argv was:\n{argv}",
    );
    assert!(
        argv.contains("local=local.vcf:+2"),
        "the second is the local side, which the source contributed; argv was:\n{argv}",
    );
    assert!(
        argv.contains("remote=remote.vcf:+3"),
        "the third is the remote side, the target's own; argv was:\n{argv}",
    );
    assert!(
        argv.contains("output=merged.vcf:0"),
        "the fourth is an empty path to write, not a fourth body; argv was:\n{argv}",
    );
    assert!(
        argv.contains(&format!("/{TMP_DIR}/")),
        "a path holding a space reached the merger whole; argv was:\n{argv}",
    );

    assert!(
        settled.contains(&format!("Settled conflict {}", parked.id))
            && settled.contains("with the merged body"),
        "the decision is taken from the merger's output; it said:\n{settled}",
    );

    // The body the merger composed is neither side, so finding it on both
    // servers proves the run pushed what it took back rather than a side it
    // could have picked on its own.
    sync(&parked, ORDER_BOOK, 0);
    for side in [SOURCE, TARGET] {
        assert!(
            get(side, ORDER_BOOK, CARD).contains("TEL:+7"),
            "{side} holds the body the merger wrote",
        );
    }
}

#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_merger_exiting_non_zero_settles_nothing_whatever_it_wrote() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let script = root.join("refuse.sh");

    // Writes a perfectly good body and then refuses. Taking it would settle a
    // divergence on the word of a tool that said it had failed.
    fs::write(
        &script,
        format!(
            "#!/bin/sh\n{write} > \"$4\"\nexit 3\n",
            write = printf_card("+8"),
        ),
    )
    .unwrap();

    let parked = park(root, REFUSE_BOOK, &format!("\"sh {}\"", script.display()));
    let aborted = resolve(&parked);

    assert!(
        aborted.contains(&format!("conflict {} is exactly as it was", parked.id)),
        "the refusal is reported as a decision nobody made; it said:\n{aborted}",
    );
    assert_untouched(&parked, REFUSE_BOOK);
}

#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_merger_exiting_zero_without_writing_settles_nothing_either() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let script = root.join("quit.sh");

    // What quitting an editor without saving looks like from the outside: the
    // three bodies read, the output left alone, and a zero exit.
    fs::write(&script, "#!/bin/sh\ncat \"$1\" \"$2\" \"$3\" > /dev/null\n").unwrap();

    let parked = park(root, QUIT_BOOK, &format!("\"sh {}\"", script.display()));
    let aborted = resolve(&parked);

    assert!(
        aborted.contains(&format!("conflict {} is exactly as it was", parked.id)),
        "a zero exit alone is not a decision; it said:\n{aborted}",
    );
    assert_untouched(&parked, QUIT_BOOK);
}

#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_merger_naming_its_output_placeholder_is_substituted_rather_than_appended() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let log = root.join("argv.log");
    let script = root.join("flag.sh");

    // A tool whose output is a flag rather than the last argument. Appending
    // the four paths on top would hand it nine arguments and write nothing
    // where it was told to.
    fs::write(
        &script,
        format!(
            "{TEL}\
             {{\n\
             echo \"argc=$#\"\n\
             echo \"flag=$1\"\n\
             echo \"output=$(basename \"$2\")\"\n\
             echo \"base=$(tel \"$3\")\"\n\
             echo \"local=$(tel \"$4\")\"\n\
             echo \"remote=$(tel \"$5\")\"\n\
             }} > '{log}'\n\
             {write} > \"$2\"\n",
            log = log.display(),
            write = printf_card("+9"),
        ),
    )
    .unwrap();

    let merger = format!(
        "[\"sh\", \"{}\", \"--out\", \"{{output}}\", \"{{base}}\", \"{{local}}\", \"{{remote}}\"]",
        script.display(),
    );
    let parked = park(root, FLAG_BOOK, &merger);
    let settled = resolve(&parked);

    let argv = fs::read_to_string(&log).expect("the merger ran and logged its argv");
    assert!(
        argv.contains("argc=5"),
        "a substituted command is handed no trailing paths; argv was:\n{argv}",
    );
    assert!(
        argv.contains("flag=--out") && argv.contains("output=merged.vcf"),
        "the output path went where the placeholder named it; argv was:\n{argv}",
    );
    assert!(
        argv.contains("base=+1") && argv.contains("local=+2") && argv.contains("remote=+3"),
        "each remaining placeholder took the body it names; argv was:\n{argv}",
    );

    assert!(
        settled.contains(&format!("Settled conflict {}", parked.id)),
        "the body written through the placeholder is taken; it said:\n{settled}",
    );

    sync(&parked, FLAG_BOOK, 0);
    for side in [SOURCE, TARGET] {
        assert!(
            get(side, FLAG_BOOK, CARD).contains("TEL:+9"),
            "{side} holds the body the merger wrote",
        );
    }
}

/// One test's account, and the divergence [`park`] left waiting in it.
struct Parked {
    /// The account configuration file every command reads.
    config: PathBuf,
    /// The state directory holding the account's store.
    state: PathBuf,
    /// The id `conflict resolve` addresses the divergence by.
    id: i64,
}

/// The directory name every temporary path a command builds sits under.
///
/// It holds a space on purpose: the shell form of a merger is a command line,
/// so an unquoted export path would reach the merger as two arguments.
const TMP_DIR: &str = "tmp dir";

/// Parks a real content divergence in `book` and returns what settles it.
///
/// One card, seeded on the source and crossed by a first sync, then set to a
/// value of its own on each principal: both sides moved away from a body they
/// agreed on, which is the only shape with a base to export.
fn park(root: &Path, book: &str, merger: &str) -> Parked {
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(root.join(TMP_DIR)).unwrap();
    fs::write(&config, account(merger)).unwrap();

    create_book(root, SOURCE, book);
    create_book(root, TARGET, book);
    put(root, SOURCE, book, &card("+1"));

    neverest(&["init", "-a", ACCOUNT], &config, &state, 0);
    neverest(
        &["sync", "-a", ACCOUNT, "-m", book, "--json"],
        &config,
        &state,
        0,
    );

    put(root, SOURCE, book, &card("+2"));
    put(root, TARGET, book, &card("+3"));

    let report = neverest(
        &["sync", "-a", ACCOUNT, "-m", book, "--json"],
        &config,
        &state,
        2,
    );
    assert!(
        report.contains(r#""outstandingConflicts":1"#),
        "the divergence is parked and counted; report was:\n{report}",
    );

    let listed = neverest(
        &["conflict", "list", "-a", ACCOUNT, "--json"],
        &config,
        &state,
        0,
    );
    let listed: serde_json::Value = serde_json::from_str(&listed).expect("conflict listing");
    let conflicts = listed["conflicts"].as_array().expect("a conflict array");
    assert_eq!(conflicts.len(), 1, "one card, one divergence: {listed}");
    assert_eq!(
        conflicts[0]["resolvable"], true,
        "the merger is handed three bodies, so all three are held: {listed}",
    );

    let id = conflicts[0]["id"].as_i64().expect("a conflict id");

    Parked { config, state, id }
}

/// Settles the parked divergence through the account's merger, and returns
/// what the command said about it.
fn resolve(parked: &Parked) -> String {
    let id = parked.id.to_string();

    neverest(
        &["conflict", "resolve", "-a", ACCOUNT, &id, "-i"],
        &parked.config,
        &parked.state,
        0,
    )
}

/// Asserts a merger settled nothing: the divergence is still waiting and each
/// endpoint still holds the body it went in with.
fn assert_untouched(parked: &Parked, book: &str) {
    let listed = neverest(
        &["conflict", "list", "-a", ACCOUNT, "--json"],
        &parked.config,
        &parked.state,
        0,
    );
    assert!(
        listed.contains(&format!("{CARD}.vcf")),
        "the divergence is still waiting for a decision; listing was:\n{listed}",
    );

    assert!(
        get(SOURCE, book, CARD).contains("TEL:+2"),
        "the source keeps its own edit",
    );
    assert!(
        get(TARGET, book, CARD).contains("TEL:+3"),
        "the target keeps its own edit",
    );

    // And a run over it is still a run that left something waiting, which is
    // what a supervisor reads: an abandoned merger changes no exit code.
    sync(parked, book, 2);
}

/// The account every test runs: one source and one target, each a principal
/// of the same Radicale, settling divergences through `merger`.
fn account(merger: &str) -> String {
    format!(
        "[accounts.{ACCOUNT}]\n\
         retain = true\n\
         conflict.merger = {merger}\n\
         sources.a.carddav.server = \"{DAV}/\"\n\
         sources.a.carddav.auth.basic.username = \"{SOURCE}\"\n\
         sources.a.carddav.auth.basic.password.raw = \"{SOURCE}\"\n\
         targets.b.carddav.server = \"{DAV}/\"\n\
         targets.b.carddav.auth.basic.username = \"{TARGET}\"\n\
         targets.b.carddav.auth.basic.password.raw = \"{TARGET}\"\n",
    )
}

/// Syncs the account, narrowed to one address book.
fn sync(parked: &Parked, book: &str, code: i32) -> String {
    neverest(
        &["sync", "-a", ACCOUNT, "-m", book, "--json"],
        &parked.config,
        &parked.state,
        code,
    )
}

/// Recreates a principal's address book, so a run starts from an empty one
/// whatever the previous run left behind.
///
/// Radicale does not create a collection on a member write (it answers the PUT
/// with a 409), and the container keeps its storage between runs, so the book
/// is deleted and made again with an explicit extended `MKCOL`.
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

    let body = root.join(format!("mkcol-{user}-{book}.xml"));
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

/// Writes the card to one principal's address book.
fn put(root: &Path, user: &str, book: &str, body: &str) {
    let file = root.join(format!("{user}-{book}.vcf"));
    fs::write(&file, body).unwrap();

    let output = Command::new("curl")
        .args(["-fsS", "-X", "PUT", "-u", &format!("{user}:{user}")])
        .args(["-H", "Content-Type: text/vcard; charset=utf-8"])
        .arg("--data-binary")
        .arg(format!("@{}", file.display()))
        .arg(format!("{DAV}/{user}/{book}/{CARD}.vcf"))
        .output()
        .expect("spawn curl put");

    assert!(
        output.status.success(),
        "PUT {user}/{book}/{CARD} failed: {}",
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

/// Runs neverest and checks it ended on `code`, 2 being a run that reconciled
/// and left a decision waiting.
///
/// Every temporary path the command builds is pointed at a directory whose
/// name holds a space, the merger's export directory included.
fn neverest(args: &[&str], config: &Path, state: &Path, code: i32) -> String {
    let root = config.parent().expect("the config sits in the test root");

    let output = Command::new(env!("CARGO_BIN_EXE_neverest"))
        .args(["-c", &config.to_string_lossy()])
        .args(args)
        .env("XDG_STATE_HOME", state)
        .env("TMPDIR", root.join(TMP_DIR))
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
