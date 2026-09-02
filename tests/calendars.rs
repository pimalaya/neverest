//! End-to-end test of the **iCalendar** half of the three-way merge, against a
//! local Radicale (`tests/radicale.sh`) holding two principals. Ignored by
//! default.
//!
//! tests/endpoints.rs drives the same two-endpoint account through a merge, a
//! collision and a resolution, but every body it exchanges is a vCard, so
//! `Kind::Ical` and the ical-rs merge it dispatches to have never run against a
//! server. These three do exactly that, and add the rule no card can state:
//!
//!   1. Disjoint edits on two endpoints merge. One event, one endpoint moving
//!      its `SUMMARY` and the other its `LOCATION`, is not a disagreement, and
//!      both edits must reach both servers.
//!   2. A same-field collision parks and is never overwritten. Two endpoints
//!      setting the same `SUMMARY` differently is a disagreement no merge
//!      settles, so the run ends conflicted, leaves each server holding its own
//!      body however many reruns go by, and waits for `conflict resolve`.
//!   3. A recurring series and its overrides are one item. RFC 4791 §4.1 keeps
//!      every component sharing a `UID` in one calendar object resource, so a
//!      master `VEVENT` carrying an `RRULE` and its `RECURRENCE-ID` override
//!      are one item, and a merge over them must keep both components rather
//!      than key on the component and mint two.
//!
//! An account naming a source and a target keys the store under the source's
//! name, so a calendar reads as `a/<name>` here rather than as the
//! `caldav/<name>` the direct-backend sugar of tests/caldav.rs uses.
//!
//! Each test owns a calendar of its own on both principals and narrows the run
//! to it, so the three run side by side and none meets what the others seeded.
//!
//! Start the server and run with:
//! ```sh
//! ./tests/radicale.sh
//! cargo test --features dav --test calendars -- --ignored --test-threads=1
//! ```

use std::{fs, path::Path, process::Command};

const DAV: &str = "http://127.0.0.1:5232";
/// The calendar the disjoint-merge test owns on both principals.
const MERGE_CAL: &str = "neverest-merge";
/// The calendar the collision test owns on both principals.
const CLASH_CAL: &str = "neverest-clash";
/// The calendar the recurrence test owns on both principals.
const SERIES_CAL: &str = "neverest-series";
/// The source endpoint's principal, whose password is its own name.
const SOURCE: &str = "test";
/// The target endpoint's principal.
const TARGET: &str = "test2";

/// A single event, addressed on the server by `<uid>.ics`, carrying the two
/// fields each endpoint edits one of.
fn event(uid: &str, summary: &str, location: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\n\
         VERSION:2.0\r\n\
         PRODID:-//pimalaya//neverest tests//EN\r\n\
         BEGIN:VEVENT\r\n\
         UID:{uid}\r\n\
         DTSTAMP:20260101T000000Z\r\n\
         DTSTART:20260907T090000Z\r\n\
         DTEND:20260907T093000Z\r\n\
         SUMMARY:{summary}\r\n\
         LOCATION:{location}\r\n\
         END:VEVENT\r\n\
         END:VCALENDAR\r\n",
    )
}

/// A weekly series and one override of it in the **same** resource, which is
/// what RFC 4791 §4.1 requires of components sharing a `UID`. `summary` is the
/// master's and `location` the override's, so two endpoints can each edit a
/// field in a different component of one item.
fn series(uid: &str, summary: &str, location: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\n\
         VERSION:2.0\r\n\
         PRODID:-//pimalaya//neverest tests//EN\r\n\
         BEGIN:VEVENT\r\n\
         UID:{uid}\r\n\
         DTSTAMP:20260101T000000Z\r\n\
         DTSTART:20260907T090000Z\r\n\
         DTEND:20260907T093000Z\r\n\
         RRULE:FREQ=WEEKLY;COUNT=4\r\n\
         SUMMARY:{summary}\r\n\
         LOCATION:Room A\r\n\
         END:VEVENT\r\n\
         BEGIN:VEVENT\r\n\
         UID:{uid}\r\n\
         RECURRENCE-ID:20260914T090000Z\r\n\
         DTSTAMP:20260101T000000Z\r\n\
         DTSTART:20260914T100000Z\r\n\
         DTEND:20260914T103000Z\r\n\
         SUMMARY:Stand-up moved\r\n\
         LOCATION:{location}\r\n\
         END:VEVENT\r\n\
         END:VCALENDAR\r\n",
    )
}

/// Proves the ical three-way merge runs end to end: an event each endpoint
/// edited in a field the other left alone merges, both edits reach both
/// servers, and the run ends clean with nothing waiting.
#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn disjoint_edits_on_two_calendar_endpoints_merge_into_one_event() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account()).unwrap();

    create_calendar(root, SOURCE, MERGE_CAL);
    create_calendar(root, TARGET, MERGE_CAL);

    // Seeded on one endpoint only, so the first sync binds the event on both
    // and leaves the store holding the body both sides then edit from.
    put(
        root,
        SOURCE,
        MERGE_CAL,
        "event-1",
        &event("event-1", "Stand-up", "Room A"),
    );

    neverest(&["init", "-a", "cal"], &config, &state, 0);
    sync(&config, &state, MERGE_CAL, 0);
    assert!(
        get(TARGET, MERGE_CAL, "event-1").contains("SUMMARY:Stand-up"),
        "the seed crossed to the target",
    );

    // One endpoint moves the summary, the other the location. Neither touched
    // what the other did, so the base settles it and no person is needed.
    put(
        root,
        SOURCE,
        MERGE_CAL,
        "event-1",
        &event("event-1", "Stand-up rescheduled", "Room A"),
    );
    put(
        root,
        TARGET,
        MERGE_CAL,
        "event-1",
        &event("event-1", "Stand-up", "Room B"),
    );

    let report = sync(&config, &state, MERGE_CAL, 0);
    assert!(
        report.contains(r#""outstandingConflicts":0"#),
        "a merged edit leaves nothing waiting; report was:\n{report}",
    );

    for side in [SOURCE, TARGET] {
        let merged = get(side, MERGE_CAL, "event-1");
        assert!(
            merged.contains("SUMMARY:Stand-up rescheduled") && merged.contains("LOCATION:Room B"),
            "{side} holds the merged event; it held:\n{merged}",
        );
    }

    // And the merged body is one event, not one per contributor.
    let items = store_items(&state, MERGE_CAL);
    assert_eq!(
        items.matches(r#""link_id""#).count(),
        1,
        "one identity is one item; store held:\n{items}",
    );
}

/// Proves a disagreement over one calendar field parks instead of resolving:
/// the run ends conflicted and names it, each server keeps its own body over
/// any number of reruns, and a `conflict resolve` decision converges both
/// endpoints on the next run.
#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_same_field_collision_on_two_calendar_endpoints_parks_and_never_overwrites() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account()).unwrap();

    create_calendar(root, SOURCE, CLASH_CAL);
    create_calendar(root, TARGET, CLASH_CAL);

    put(
        root,
        SOURCE,
        CLASH_CAL,
        "event-1",
        &event("event-1", "Stand-up", "Room A"),
    );

    neverest(&["init", "-a", "cal"], &config, &state, 0);
    sync(&config, &state, CLASH_CAL, 0);

    // The same field, set two ways. There is a base, and it says both sides
    // moved the summary, which is the residual no merge settles.
    put(
        root,
        SOURCE,
        CLASH_CAL,
        "event-1",
        &event("event-1", "Stand-up am", "Room A"),
    );
    put(
        root,
        TARGET,
        CLASH_CAL,
        "event-1",
        &event("event-1", "Stand-up pm", "Room A"),
    );

    let report = sync(&config, &state, CLASH_CAL, 2);
    assert!(
        report.contains(r#""outstandingConflicts":1"#),
        "the divergence is parked and counted, not resolved; report was:\n{report}",
    );
    assert!(
        report.contains(r#""id":"event-1.ics""#),
        "the parked item is named; report was:\n{report}",
    );

    assert!(
        get(SOURCE, CLASH_CAL, "event-1").contains("SUMMARY:Stand-up am"),
        "the source keeps its own edit",
    );
    assert!(
        get(TARGET, CLASH_CAL, "event-1").contains("SUMMARY:Stand-up pm"),
        "the target keeps its own edit, which is the body a run would have \
         replaced",
    );

    // Rerunning settles nothing on its own and overwrites nothing either: a
    // parked divergence waits for a person, however many runs go by.
    let report = sync(&config, &state, CLASH_CAL, 2);
    assert!(
        !report.contains(r#""kind":"update""#),
        "a rerun pushes neither body over the other; report was:\n{report}",
    );
    assert!(
        get(SOURCE, CLASH_CAL, "event-1").contains("SUMMARY:Stand-up am")
            && get(TARGET, CLASH_CAL, "event-1").contains("SUMMARY:Stand-up pm"),
        "both edits survive a rerun",
    );

    // It is a decision the conflict commands can find and settle, and the
    // local body is the one the store holds, which the source contributed.
    let listed = neverest(
        &["conflict", "list", "-a", "cal", "--json"],
        &config,
        &state,
        0,
    );
    let id = conflict_id(&listed, "event-1.ics").to_string();

    neverest(
        &["conflict", "resolve", "-a", "cal", &id, "--prefer-local"],
        &config,
        &state,
        0,
    );

    // One run then carries the decision to both endpoints.
    sync(&config, &state, CLASH_CAL, 0);
    for side in [SOURCE, TARGET] {
        assert!(
            get(side, CLASH_CAL, "event-1").contains("SUMMARY:Stand-up am"),
            "{side} converged on the body --prefer-local settled on; it held:\n{}",
            get(side, CLASH_CAL, "event-1"),
        );
    }
}

/// Proves the item is the calendar object **resource** rather than the
/// component (RFC 4791 §4.1): a master `VEVENT` and its `RECURRENCE-ID`
/// override sharing a `UID` are one item, an edit to each component from a
/// different endpoint merges, and no component is dropped on the way.
#[test]
#[ignore = "requires a Radicale instance (./tests/radicale.sh) on :5232 and --ignored"]
fn a_recurring_series_and_its_override_stay_one_item_through_a_merge() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path();
    let state = root.join("state");
    let config = root.join("config.toml");
    fs::create_dir_all(&state).unwrap();
    fs::write(&config, account()).unwrap();

    create_calendar(root, SOURCE, SERIES_CAL);
    create_calendar(root, TARGET, SERIES_CAL);

    put(
        root,
        SOURCE,
        SERIES_CAL,
        "series-1",
        &series("series-1", "Stand-up", "Room A"),
    );

    neverest(&["init", "-a", "cal"], &config, &state, 0);
    sync(&config, &state, SERIES_CAL, 0);

    // Two components, one UID, one resource: the store must hold one item,
    // keyed by that UID, not one per component.
    let items = store_items(&state, SERIES_CAL);
    assert!(
        items.contains(r#""link_id":"series-1""#),
        "the series is keyed by its UID; store held:\n{items}",
    );
    assert_eq!(
        items.matches(r#""link_id""#).count(),
        1,
        "a series and its override are ONE item; store held:\n{items}",
    );

    // The whole resource crossed, override included.
    let crossed = get(TARGET, SERIES_CAL, "series-1");
    assert!(
        crossed.contains("RRULE:FREQ=WEEKLY") && crossed.contains("RECURRENCE-ID:20260914T090000Z"),
        "both components crossed to the target; it held:\n{crossed}",
    );

    // One endpoint edits the master, the other the override. Different
    // components, different fields: nobody disagreed.
    put(
        root,
        SOURCE,
        SERIES_CAL,
        "series-1",
        &series("series-1", "Stand-up weekly", "Room A"),
    );
    put(
        root,
        TARGET,
        SERIES_CAL,
        "series-1",
        &series("series-1", "Stand-up", "Room B"),
    );

    let report = sync(&config, &state, SERIES_CAL, 0);
    assert!(
        report.contains(r#""outstandingConflicts":0"#),
        "edits to two components of one resource are not a disagreement; \
         report was:\n{report}",
    );

    let items = store_items(&state, SERIES_CAL);
    assert_eq!(
        items.matches(r#""link_id""#).count(),
        1,
        "the merge kept one item rather than minting one per component; store \
         held:\n{items}",
    );

    for side in [SOURCE, TARGET] {
        let merged = get(side, SERIES_CAL, "series-1");
        assert_eq!(
            merged.matches("BEGIN:VEVENT").count(),
            2,
            "{side} still holds both components; it held:\n{merged}",
        );

        // Each edit landed in the component it was made in, rather than
        // anywhere in the resource: a merge keying on the UID alone would fold
        // the override into the master and lose one of the two.
        let master = component(&merged, "RRULE:FREQ=WEEKLY");
        assert!(
            master.contains("SUMMARY:Stand-up weekly") && master.contains("LOCATION:Room A"),
            "{side} carries the master's edit in the master; it held:\n{merged}",
        );

        let override_ = component(&merged, "RECURRENCE-ID:20260914T090000Z");
        assert!(
            override_.contains("LOCATION:Room B") && override_.contains("SUMMARY:Stand-up moved"),
            "{side} carries the override's edit in the override; it held:\n{merged}",
        );
    }
}

/// The `VEVENT` of `ics` holding `marker`, so an assertion can name which
/// component a property ended up in.
fn component<'a>(ics: &'a str, marker: &str) -> &'a str {
    ics.split("BEGIN:VEVENT")
        .find(|component| component.contains(marker))
        .unwrap_or_else(|| panic!("no component holding {marker} in:\n{ics}"))
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

/// The account every test runs: one source and one target, each a CalDAV
/// principal of the same Radicale, keeping a local copy of what crosses.
fn account() -> String {
    format!(
        "[accounts.cal]\n\
         retain = true\n\
         sources.a.caldav.server = \"{DAV}/\"\n\
         sources.a.caldav.auth.basic.username = \"{SOURCE}\"\n\
         sources.a.caldav.auth.basic.password.raw = \"{SOURCE}\"\n\
         targets.b.caldav.server = \"{DAV}/\"\n\
         targets.b.caldav.auth.basic.username = \"{TARGET}\"\n\
         targets.b.caldav.auth.basic.password.raw = \"{TARGET}\"\n",
    )
}

/// Recreates a principal's calendar, so a run starts from an empty one whatever
/// the previous run left behind.
///
/// Radicale does not create a collection on a member write (it answers the PUT
/// with a 409), and the container keeps its storage between runs, so the
/// calendar is deleted and made again with an explicit `MKCALENDAR`.
fn create_calendar(root: &Path, user: &str, calendar: &str) {
    let output = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-X", "DELETE"])
        .args(["-u", &format!("{user}:{user}")])
        .arg(format!("{DAV}/{user}/{calendar}/"))
        .output()
        .expect("spawn curl delete calendar");
    assert!(
        output.status.success(),
        "DELETE of {user}'s calendar failed",
    );

    let body = root.join(format!("mkcalendar-{user}-{calendar}.xml"));
    fs::write(
        &body,
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <C:mkcalendar xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\">\
             <D:set><D:prop>\
             <D:displayname>{calendar}</D:displayname>\
             </D:prop></D:set></C:mkcalendar>",
        ),
    )
    .unwrap();

    let output = Command::new("curl")
        .args(["-fsS", "-X", "MKCALENDAR", "-u", &format!("{user}:{user}")])
        .args(["-H", "Content-Type: application/xml; charset=utf-8"])
        .arg("--data-binary")
        .arg(format!("@{}", body.display()))
        .arg(format!("{DAV}/{user}/{calendar}/"))
        .output()
        .expect("spawn curl mkcalendar");

    assert!(
        output.status.success(),
        "MKCALENDAR {user}/{calendar} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Writes a calendar object resource to one principal's calendar.
fn put(root: &Path, user: &str, calendar: &str, id: &str, body: &str) {
    let file = root.join(format!("{user}-{calendar}-{id}.ics"));
    fs::write(&file, body).unwrap();

    let output = Command::new("curl")
        .args(["-fsS", "-X", "PUT", "-u", &format!("{user}:{user}")])
        .args(["-H", "Content-Type: text/calendar; charset=utf-8"])
        .arg("--data-binary")
        .arg(format!("@{}", file.display()))
        .arg(format!("{DAV}/{user}/{calendar}/{id}.ics"))
        .output()
        .expect("spawn curl put");

    assert!(
        output.status.success(),
        "PUT {user}/{calendar}/{id} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Reads a calendar object resource back from one principal's calendar.
fn get(user: &str, calendar: &str, id: &str) -> String {
    let output = Command::new("curl")
        .args(["-fsS", "-u", &format!("{user}:{user}")])
        .arg(format!("{DAV}/{user}/{calendar}/{id}.ics"))
        .output()
        .expect("spawn curl get");

    assert!(
        output.status.success(),
        "GET {user}/{calendar}/{id} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Syncs the account, narrowed to one calendar.
fn sync(config: &Path, state: &Path, calendar: &str, code: i32) -> String {
    neverest(
        &["sync", "-a", "cal", "-m", calendar, "--json"],
        config,
        state,
        code,
    )
}

/// The store's live items for a calendar, as the `pimdir` CLI renders them.
fn store_items(state: &Path, calendar: &str) -> String {
    let store = state.join("neverest").join("cal");
    let output = Command::new("pimdir")
        .args(["--store", &store.to_string_lossy()])
        .args(["item", "list", &format!("a/{calendar}"), "--json"])
        .output()
        .expect("spawn pimdir (cargo install --path ../io-pimdir --features cli)");

    assert!(
        output.status.success(),
        "`pimdir item list a/{calendar}` failed:\n{}",
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
