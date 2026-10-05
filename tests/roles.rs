//! Collection roles recorded at sync, and `sync --declare-only`, against a
//! local Stalwart (server A: IMAP on :143, HTTP and DAV on :8080), spawned
//! via `tests/stalwart2.sh`. Ignored by default.
//!
//! What a frontend reads from the store to know the inbox, the sent folder
//! or the default calendar (pimdir STORAGE §14), without asking the server.
//!
//! Start the servers and run with:
//! ```sh
//! ./tests/stalwart2.sh
//! cargo test --test roles -- --ignored --test-threads=1
//! ```

use std::{collections::BTreeMap, fs, path::Path, process::Command};

use io_pimdir::client::reader::PimdirReader;

const IMAP_ROOT: &str = "imap://127.0.0.1:143";
const DAV: &str = "http://127.0.0.1:8080/dav";
const USER: &str = "test@pimalaya.org";
const PASS: &str = "P!malaya-test-2026";

#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :143 and --ignored"]
fn a_declaration_records_every_folder_and_its_role_and_fetches_nothing() {
    let (tmp, config) = account(&format!(
        "imap.server = \"{IMAP_ROOT}\"\n\
         imap.starttls = false\n\
         imap.sasl.plain.username = \"{USER}\"\n\
         imap.sasl.plain.password.raw = \"{PASS}\"\n\
         imap.collection.filter.include = [\"INBOX\"]\n",
    ));
    let state = tmp.path().join("state");
    neverest(&["init", "-a", "roles"], &config, &state);

    neverest(&["sync", "-a", "roles", "--declare-only"], &config, &state);
    let store = state.join("neverest").join("roles");
    let roles = roles(&store);
    assert_eq!(roles["imap/INBOX"].as_deref(), Some("inbox"), "{roles:?}");
    assert_eq!(
        roles["imap/Sent Items"].as_deref(),
        Some("sent"),
        "{roles:?}"
    );
    assert_eq!(roles["imap/Drafts"].as_deref(), Some("drafts"), "{roles:?}");
    assert_eq!(
        roles["imap/Deleted Items"].as_deref(),
        Some("trash"),
        "{roles:?}"
    );
    assert_eq!(
        roles["imap/Junk Mail"].as_deref(),
        Some("junk"),
        "{roles:?}"
    );

    let reader = PimdirReader::open(&store).unwrap();
    for id in roles.keys() {
        assert_eq!(reader.count_items(id).unwrap(), 0, "{id}: no item moves");
    }

    // A regular sync, narrowed by the filter, records the roles of what it
    // syncs and leaves the other collections declared.
    neverest(&["sync", "-a", "roles"], &config, &state);
    let after = self::roles(&store);
    assert_eq!(after, roles, "the same roles");
}

#[test]
#[ignore = "requires a Stalwart instance (./tests/stalwart2.sh) on :8080 and --ignored"]
fn a_declaration_records_the_default_calendar_the_server_states() {
    let (tmp, config) = account(&format!(
        "caldav.server = \"{DAV}/cal/\"\n\
         caldav.auth.basic.username = \"{USER}\"\n\
         caldav.auth.basic.password.raw = \"{PASS}\"\n",
    ));
    let state = tmp.path().join("state");
    neverest(&["init", "-a", "roles"], &config, &state);
    neverest(&["sync", "-a", "roles", "--declare-only"], &config, &state);

    let store = state.join("neverest").join("roles");
    let roles = roles(&store);
    let defaults: Vec<&String> = roles
        .iter()
        .filter(|(_, role)| role.as_deref() == Some("default"))
        .map(|(id, _)| id)
        .collect();
    assert_eq!(defaults, ["caldav/default"], "{roles:?}");

    // The second run reuses the looked-up default rather than asking again,
    // and keeps it.
    let sidecar = fs::read_to_string(store.join("neverest.json")).unwrap();
    assert!(sidecar.contains("looked_up_roles"), "{sidecar}");
    neverest(&["sync", "-a", "roles", "-m", "default"], &config, &state);
    assert_eq!(self::roles(&store), roles);
}

/// A one-source account named `roles` with `body`, and its config path.
fn account(body: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    fs::create_dir_all(tmp.path().join("state")).unwrap();
    let config = tmp.path().join("config.toml");
    fs::write(&config, format!("[accounts.roles]\n{body}")).unwrap();
    (tmp, config)
}

/// Every collection the store holds, with its role.
fn roles(store: &Path) -> BTreeMap<String, Option<String>> {
    PimdirReader::open(store)
        .unwrap()
        .list_collections()
        .unwrap()
        .into_iter()
        .map(|collection| (collection.id, collection.role))
        .collect()
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
