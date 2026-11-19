//! # CalDAV
//!
//! End-to-end CalDAV test against a local Radicale (tests/radicale.sh).
//! Ignored by default.
//!
//! The DAV adapter is shared with the CardDAV twin, so this run is for what
//! it does not share (calendar home set and listing, `calendar-multiget`,
//! the `.ics` names a `UID` maps to) and the calendar kind io-pimdir derives:
//!
//! - two events seeded (`curl` PUT) land keyed by `UID`, sorted by their
//!   start resolved to UTC; a first dry run names them already, since a DAV
//!   kind resolves its link id only at `Full`;
//! - an event edited on the server moves its ETag, so it is re-fetched;
//! - an event deleted on the server is retained, not lost;
//! - a sync with nothing changed is quiescent: a retained row re-derived
//!   would be re-uploaded on every run.
//!
//! Start the server and run with:
//! ```sh
//! ./tests/radicale.sh
//! cargo test --features caldav --test caldav -- --ignored
//! ```

mod common;

use std::{fs, path::Path, process::Command};

use crate::common::{link_ids, retained_items, store_items, summary_text};

const DAV: &str = "http://127.0.0.1:5232";
const USER: &str = "test";
const PASS: &str = "test";
const CALENDAR: &str = "agenda";
/// The calendar as the store keys it: account, source, then calendar.
///
/// For the direct-backend sugar the source name is the protocol name.
const COLLECTION: &str = "calendar/caldav/agenda";

/// An event, addressed on the server by `<uid>.ics` (the same resource name
/// the DAV adapter derives from a `UID`).
fn event(uid: &str, summary: &str, start: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\n\
         VERSION:2.0\r\n\
         PRODID:-//pimalaya//neverest tests//EN\r\n\
         BEGIN:VEVENT\r\n\
         UID:{uid}\r\n\
         DTSTAMP:20260101T000000Z\r\n\
         DTSTART:{start}\r\n\
         DTEND:{start}\r\n\
         SUMMARY:{summary}\r\n\
         END:VEVENT\r\n\
         END:VCALENDAR\r\n",
    )
}

#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_caldav_side_syncs_edits_and_retains_a_server_delete() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let store = state.join("neverest");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();

    fs::write(
        &config,
        format!(
            "[accounts.calendar]\n\
             caldav.server = \"{DAV}/\"\n\
             caldav.auth.basic.username = \"{USER}\"\n\
             caldav.auth.basic.password.raw = \"{PASS}\"\n",
        ),
    )
    .unwrap();

    create_calendar(root);
    put(
        root,
        "event-1",
        &event("event-1", "Stand-up", "20260814T090000Z"),
    );
    put(
        root,
        "event-2",
        &event("event-2", "Retro", "20260814T160000Z"),
    );

    neverest(&["init", "-a", "calendar"], &config, &state);

    let plan = neverest(&["sync", "-a", "calendar", "-d", "--json"], &config, &state);
    assert!(
        plan.contains("event-1") && plan.contains("event-2"),
        "a first dry run names both events; report was:\n{plan}",
    );

    neverest(&["sync", "-a", "calendar"], &config, &state);

    let items = store_items(&store, COLLECTION);
    assert_eq!(
        link_ids(&items),
        ["event-1", "event-2"],
        "both events landed keyed by their UID, chronologically; store held:\n{items:#?}",
    );
    assert_eq!(
        summary_text(&items[0]),
        "Stand-up",
        "the summary carries what an agenda renders; store held:\n{items:#?}",
    );

    put(
        root,
        "event-1",
        &event("event-1", "Stand-up rescheduled", "20260814T093000Z"),
    );

    let plan = neverest(&["sync", "-a", "calendar", "-d", "--json"], &config, &state);
    assert!(
        plan.contains("event-1"),
        "a dry run names the body it would re-fetch; report was:\n{plan}",
    );

    neverest(&["sync", "-a", "calendar"], &config, &state);

    let items = store_items(&store, COLLECTION);
    assert!(
        items
            .iter()
            .any(|item| summary_text(item) == "Stand-up rescheduled"),
        "the edited body reached the store; store held:\n{items:#?}",
    );

    delete("event-2");
    neverest(&["sync", "-a", "calendar"], &config, &state);

    let live = store_items(&store, COLLECTION);
    assert!(
        !link_ids(&live).contains(&"event-2"),
        "the deleted event left the live listing; store held:\n{live:#?}",
    );
    let retained = retained_items(&store, COLLECTION);
    assert!(
        link_ids(&retained).contains(&"event-2"),
        "the deleted event is retained, not lost; retained listing:\n{retained:#?}",
    );

    let report = neverest(&["sync", "-a", "calendar", "--json"], &config, &state);
    assert!(
        !report.contains("\"event-2\""),
        "a quiescent run touches nothing; report was:\n{report}",
    );
}

/// Recreates the calendar on the server, so a run starts from an empty one
/// whatever the previous run left behind.
///
/// Radicale does not create a collection on a member write (it answers the PUT
/// with a 409), and the container keeps its storage between runs, so the
/// calendar is deleted and made again with an explicit `MKCALENDAR`.
fn create_calendar(root: &Path) {
    let output = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-X", "DELETE"])
        .args(["-u", &format!("{USER}:{PASS}")])
        .arg(format!("{DAV}/{USER}/{CALENDAR}/"))
        .output()
        .expect("spawn curl delete calendar");
    assert!(output.status.success(), "DELETE of the calendar failed");

    let body = root.join("mkcalendar.xml");
    fs::write(
        &body,
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <C:mkcalendar xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\">\
         <D:set><D:prop>\
         <D:displayname>agenda</D:displayname>\
         </D:prop></D:set></C:mkcalendar>",
    )
    .unwrap();

    let output = Command::new("curl")
        .args(["-fsS", "-X", "MKCALENDAR", "-u", &format!("{USER}:{PASS}")])
        .args(["-H", "Content-Type: application/xml; charset=utf-8"])
        .arg("--data-binary")
        .arg(format!("@{}", body.display()))
        .arg(format!("{DAV}/{USER}/{CALENDAR}/"))
        .output()
        .expect("spawn curl mkcalendar");

    assert!(
        output.status.success(),
        "MKCALENDAR {CALENDAR} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Writes an event to the server.
fn put(root: &Path, id: &str, body: &str) {
    let file = root.join(format!("{id}.ics"));
    fs::write(&file, body).unwrap();

    let output = Command::new("curl")
        .args(["-fsS", "-X", "PUT", "-u", &format!("{USER}:{PASS}")])
        .args(["-H", "Content-Type: text/calendar; charset=utf-8"])
        .arg("--data-binary")
        .arg(format!("@{}", file.display()))
        .arg(format!("{DAV}/{USER}/{CALENDAR}/{id}.ics"))
        .output()
        .expect("spawn curl put");

    assert!(
        output.status.success(),
        "PUT {id} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Deletes an event on the server.
fn delete(id: &str) {
    let output = Command::new("curl")
        .args(["-fsS", "-X", "DELETE", "-u", &format!("{USER}:{PASS}")])
        .arg(format!("{DAV}/{USER}/{CALENDAR}/{id}.ics"))
        .output()
        .expect("spawn curl delete");

    assert!(
        output.status.success(),
        "DELETE {id} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
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
