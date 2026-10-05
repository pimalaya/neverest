//! Live plumbing tests against Microsoft Graph: mail, contacts and calendars,
//! on the Pimalaya test mailbox. Ignored by default.
//!
//! As in tests/google.rs, each runs two replicas of one account against one
//! throwaway collection (see tests/common). Graph creates every uploaded
//! message as a draft, so the mail tests seed theirs through the API, then
//! cross flags, a removal and moves, and add only into Drafts.
//!
//! Every resource a run creates is named `neverest-live-<millis>` and deleted
//! however the run ends. Run with the app registration's client secret (the
//! app needs `Mail.ReadWrite`, `Contacts.ReadWrite` and
//! `Calendars.ReadWrite`):
//!
//! ```sh
//! MSGRAPH_TENANT_ID=… MSGRAPH_CLIENT_ID=… MSGRAPH_CLIENT_SECRET=… \
//! cargo test --test msgraph -- --ignored --test-threads=1
//! ```

#![cfg(feature = "msgraph")]

mod common;

use std::{
    thread,
    time::{Duration, Instant},
};

use io_msgraph::v1::{
    client::{MsgraphClientStd, MsgraphClientStdConnectOptions},
    field::MsgraphField,
    rest::users::{
        calendars::MsgraphCalendar,
        contact_folders::MsgraphContactFolder,
        mail_folders::MsgraphMailFolder,
        messages::{MsgraphMessage, list::MsgraphMessagesListParams},
    },
};

use crate::common::*;

/// How long Graph may take to list what a run wrote.
const LISTED: Duration = Duration::from_secs(60);

/// The messages of the folder `folder` (an id or a well-known name)
/// whose `Message-ID` is `<tag@pimalaya.org>`, as `message` writes it.
fn messages(client: &mut MsgraphClientStd, folder: &str, tag: &str) -> Vec<MsgraphMessage> {
    let filter = format!("internetMessageId eq '<{tag}@pimalaya.org>'");
    let params = MsgraphMessagesListParams {
        filter: Some(&filter),
        select: Some("id,isRead,isDraft,internetMessageId"),
        ..Default::default()
    };
    client
        .messages_list(Some(folder), &params)
        .expect("list the folder")
        .response
        .value
}

/// Waits until the folder `folder` lists `count` messages tagged `tag`,
/// Graph's listing trailing its writes by a moment.
fn wait_count(client: &mut MsgraphClientStd, folder: &str, tag: &str, count: usize, what: &str) {
    let deadline = Instant::now() + LISTED;
    loop {
        let listed = messages(client, folder, tag).len();
        if listed == count {
            return;
        }
        if Instant::now() > deadline {
            panic!("{what}: {folder} lists {listed} copies, not {count}");
        }
        thread::sleep(Duration::from_secs(2));
    }
}

/// Deletes every message tagged `tag` in each of `folders`.
fn delete_tagged(token: &str, folders: &[&str], tag: &str) {
    let mut client = connect(token);
    for folder in folders {
        for message in messages(&mut client, folder, tag) {
            if let Err(err) = client.message_delete(&message.id) {
                eprintln!("WARNING: leftover message {tag} in {folder}: {err:?}");
            }
        }
    }
}

/// Opens a Graph client on the test mailbox.
fn connect(token: &str) -> MsgraphClientStd {
    let options = MsgraphClientStdConnectOptions {
        user_id: msgraph_user(),
        ..Default::default()
    };
    MsgraphClientStd::connect(token, options).expect("connect to Graph")
}

/// The configuration line pointing a source at the test mailbox.
fn user_line(backend: &str) -> String {
    format!("{backend}.user-id = \"{}\"", msgraph_user())
}

/// A message seeded in a folder reaches both replicas, flags cross, a fresh
/// replica reads them, and a removal deletes it.
#[test]
#[ignore = "live: needs the app registration's client secret"]
fn a_graph_mail_folder_syncs_both_ways() {
    let token = msgraph_token();
    let folder = tag();
    let mut client = connect(&token);
    let folder_id = client
        .mail_folder_create(&MsgraphMailFolder {
            display_name: folder.clone(),
            ..Default::default()
        })
        .expect("create the folder")
        .response
        .id;
    client
        .message_create_mime(
            Some(&folder_id),
            message(&folder, &msgraph_user()).as_bytes(),
        )
        .expect("seed the message");

    with_cleanup(
        || {
            let extra = user_line("msgraph");
            let a = Replica::open("msgraph", &extra, &token);
            let b = Replica::open("msgraph", &extra, &token);

            a.sync_until(&folder, "the seeded message reaches a replica", |a| {
                a.item(&folder, &folder).is_some()
            });
            b.sync_until(&folder, "and the other one", |b| {
                b.item(&folder, &folder).is_some()
            });

            let item = b.item(&folder, &folder).unwrap();
            let mut set = flags(&item);
            set.insert(String::from("\\Seen"));
            set.insert(String::from("\\Flagged"));
            let set: Vec<&str> = set.iter().map(String::as_str).collect();
            b.set_flags(&folder, item.seq, &set);
            b.sync(&folder);

            a.sync_until(&folder, "the flags reach the first replica", |a| {
                a.item(&folder, &folder).is_some_and(|item| {
                    let flags = flags(&item);
                    flags.contains("\\Seen") && flags.contains("\\Flagged")
                })
            });

            let c = Replica::open("msgraph", &extra, &token);
            c.sync(&folder);
            let item = c.item(&folder, &folder).expect("a full round reads it");
            assert!(flags(&item).contains("\\Flagged"), "with its flags");
            assert!(
                c.body(&item).contains("Sent by the neverest live tests."),
                "and its body",
            );

            let seq = a.item(&folder, &folder).unwrap().seq;
            a.remove(&folder, seq);
            a.sync(&folder);

            b.sync_until(&folder, "the removal reaches the other replica", |b| {
                b.item(&folder, &folder).is_none()
            });
        },
        || {
            if let Err(err) = connect(&token).mail_folder_delete(&folder_id) {
                eprintln!("WARNING: leftover folder {folder}: {err:?}");
            }
        },
    );
}

/// A move staged through the store lands once on Graph, whichever of its
/// halves delivers: the source relocating the message (its new id read by
/// the target's next enumeration) or the target copying it (its new id the
/// copy's answer). The message bounces between the Inbox and a throwaway
/// folder, each run checked on the server and in the store, then goes to
/// Deleted Items, the way a frontend trashes.
#[test]
#[ignore = "live: needs the app registration's client secret"]
fn a_graph_move_is_delivered_once() {
    let token = msgraph_token();
    let folder = tag();
    let mut client = connect(&token);
    let folder_id = client
        .mail_folder_create(&MsgraphMailFolder {
            display_name: folder.clone(),
            ..Default::default()
        })
        .expect("create the folder")
        .response
        .id;
    client
        .message_create_mime(Some("inbox"), message(&folder, &msgraph_user()).as_bytes())
        .expect("seed the message");

    with_cleanup(
        || {
            let extra = user_line("msgraph");
            let a = Replica::open("msgraph", &extra, &token);
            let trash = "Deleted Items";
            let keys = ["Inbox", folder.as_str(), trash];

            let deadline = Instant::now() + LISTED;
            while a.item("Inbox", &folder).is_none() {
                assert!(
                    Instant::now() < deadline,
                    "the seeded message never arrived"
                );
                a.sync_keys(&keys, &[0]);
            }
            let seq = a.item("Inbox", &folder).unwrap().seq;

            let mut from = ("Inbox", "inbox");
            let mut to = (folder.as_str(), folder_id.as_str());
            for bounce in 1..=4 {
                a.move_to(from.0, seq, to.0);
                a.sync_keys(&keys, &[0]);

                let what = format!("bounce {bounce}");
                wait_count(&mut client, to.1, &folder, 1, &what);
                wait_count(&mut client, from.1, &folder, 0, &what);

                // NOTE: a second run proves the store bound the server's
                // copy: a create still pending would push again.
                a.sync_keys(&keys, &[0]);
                let landed = a.items(to.0, &folder);
                assert_eq!(landed.len(), 1, "{what}: {} holds it once", to.0);
                assert_eq!(landed[0].seq, seq, "{what}: under the same seq");
                assert!(
                    a.items(from.0, &folder).is_empty(),
                    "{what}: {} is empty",
                    from.0
                );
                wait_count(&mut client, to.1, &folder, 1, &what);

                (from, to) = (to, from);
            }

            a.move_to(from.0, seq, trash);
            a.sync_keys(&keys, &[0]);
            wait_count(&mut client, "deleteditems", &folder, 1, "trash");
            wait_count(&mut client, from.1, &folder, 0, "trash");
            a.sync_keys(&keys, &[0]);
            let trashed = a.items(trash, &folder);
            assert_eq!(trashed.len(), 1, "Deleted Items holds it once");
            assert_eq!(trashed[0].seq, seq, "under the same seq");

            let b = Replica::open("msgraph", &extra, &token);
            b.sync_keys(&keys, &[0]);
            assert_eq!(
                b.items(trash, &folder).len(),
                1,
                "a fresh replica reads it once"
            );
            assert!(b.items("Inbox", &folder).is_empty());
            assert!(b.items(&folder, &folder).is_empty());
        },
        || {
            delete_tagged(&token, &["inbox", "deleteditems"], &folder);
            if let Err(err) = connect(&token).mail_folder_delete(&folder_id) {
                eprintln!("WARNING: leftover folder {folder}: {err:?}");
            }
        },
    );
}

/// A message added through the store lands in Drafts with its flags, bound
/// to the id Graph gave it; an add anywhere else is rejected, not filed as
/// a draft.
#[test]
#[ignore = "live: needs the app registration's client secret"]
fn a_graph_add_lands_in_drafts_only() {
    let token = msgraph_token();
    let draft = tag();
    let elsewhere = format!("{draft}-elsewhere");
    let mut client = connect(&token);
    let folder_id = client
        .mail_folder_create(&MsgraphMailFolder {
            display_name: draft.clone(),
            ..Default::default()
        })
        .expect("create the folder")
        .response
        .id;

    with_cleanup(
        || {
            let extra = user_line("msgraph");
            let a = Replica::open("msgraph", &extra, &token);
            a.sync_keys(&["Drafts"], &[0]);

            a.add(
                "Drafts",
                &message(&draft, &msgraph_user()),
                &["\\Draft", "\\Seen"],
            );
            a.sync_keys(&["Drafts"], &[0]);
            wait_count(&mut client, "drafts", &draft, 1, "the add");
            let created = &messages(&mut client, "drafts", &draft)[0];
            assert_eq!(created.is_draft, Some(true), "Graph files it as a draft");
            assert_eq!(
                created.is_read,
                Some(true),
                "with the flag it was added with"
            );

            let seq = a.item("Drafts", &draft).expect("the added item stays").seq;
            a.sync_keys(&["Drafts"], &[0]);
            let held = a.items("Drafts", &draft);
            assert_eq!(held.len(), 1, "a second run pushes nothing more");
            assert_eq!(held[0].seq, seq);
            wait_count(&mut client, "drafts", &draft, 1, "the second run");

            let b = Replica::open("msgraph", &extra, &token);
            b.sync_keys(&["Drafts"], &[0]);
            let item = b.item("Drafts", &draft).expect("a fresh replica reads it");
            let set = flags(&item);
            assert!(set.contains("\\Draft") && set.contains("\\Seen"), "{set:?}");

            a.sync_keys(&[&draft], &[0]);
            a.add(&draft, &message(&elsewhere, &msgraph_user()), &[]);
            a.sync_keys(&[&draft], &[2]);
            assert!(
                messages(&mut client, &folder_id, &elsewhere).is_empty(),
                "nothing is filed outside Drafts"
            );
            assert!(
                a.item(&draft, &elsewhere).is_some(),
                "the rejected add stays pending"
            );
        },
        || {
            delete_tagged(&token, &["drafts"], &draft);
            if let Err(err) = connect(&token).mail_folder_delete(&folder_id) {
                eprintln!("WARNING: leftover folder {draft}: {err:?}");
            }
        },
    );
}

/// A card crosses both ways through a throwaway contact folder.
#[test]
#[ignore = "live: needs the app registration's client secret"]
fn a_graph_contact_folder_syncs_both_ways() {
    let token = msgraph_token();
    let folder = connect(&token)
        .contact_folder_create(&MsgraphContactFolder {
            display_name: tag(),
            ..Default::default()
        })
        .expect("create the contact folder")
        .response
        .id;

    with_cleanup(
        || {
            let extra = user_line("msgraph-contacts");
            round_trip("msgraph-contacts", &extra, &token, &folder, card);
        },
        || {
            if let Err(err) = connect(&token).contact_folder_delete(&folder) {
                eprintln!("WARNING: leftover contact folder {folder}: {err:?}");
            }
        },
    );
}

/// An event crosses both ways through a throwaway calendar.
#[test]
#[ignore = "live: needs the app registration's client secret"]
fn a_graph_calendar_syncs_both_ways() {
    let token = msgraph_token();
    let calendar = connect(&token)
        .calendar_create(&MsgraphCalendar {
            name: MsgraphField::Set(tag()),
            ..Default::default()
        })
        .expect("create the calendar")
        .response
        .id;

    with_cleanup(
        || {
            let extra = user_line("msgraph-calendar");
            round_trip("msgraph-calendar", &extra, &token, &calendar, event);
        },
        || {
            if let Err(err) = connect(&token).calendar_delete(&calendar) {
                eprintln!("WARNING: leftover calendar {calendar}: {err:?}");
            }
        },
    );
}

/// Every calendar of the mailbox syncs, the ones Graph makes itself
/// (Birthdays, holidays) among them: their series have no end, which Graph
/// writes as `0001-01-01`, and a window ending there was refused on every
/// run. A run that cannot read a calendar now exits 3.
#[test]
#[ignore = "live: needs the app registration's client secret"]
fn every_graph_calendar_of_the_mailbox_syncs() {
    let token = msgraph_token();
    let extra = user_line("msgraph-calendar");
    let a = Replica::open("msgraph-calendar", &extra, &token);
    a.sync_keys(&[], &[0]);
}

/// A check names the folders Graph knows by their use, whatever the
/// mailbox's language calls them, and the Drafts folder among them: the one
/// place Graph creates a message.
#[test]
#[ignore = "live: needs the app registration's client secret"]
fn a_graph_check_names_the_well_known_folders() {
    let token = msgraph_token();
    let report = Replica::open("msgraph", &user_line("msgraph"), &token).check();
    let collections = report["sources"][0]["collections"]
        .as_array()
        .unwrap_or_else(|| panic!("collections listed: {report}"));
    let mut client = connect(&token);

    for (well_known, role) in [
        ("inbox", "inbox"),
        ("drafts", "drafts"),
        ("sentitems", "sent"),
        ("deleteditems", "trash"),
    ] {
        let name = client
            .mail_folder_get(well_known)
            .expect("get the well-known folder")
            .response
            .display_name;
        let stated: Vec<&str> = collections
            .iter()
            .filter(|collection| collection["role"] == role)
            .filter_map(|collection| collection["id"].as_str())
            .collect();
        assert_eq!(stated, [name.as_str()], "{role}: {report}");
    }
}

/// The join link of a Teams meeting created on Graph reaches the store.
///
/// A tenant without Teams gives the meeting no link: the test then says it
/// was skipped and proves nothing (the Pimalaya test tenant, 2026-10-04).
#[test]
#[ignore = "live: needs the app registration's client secret"]
fn a_graph_teams_meeting_carries_its_join_link() {
    let token = msgraph_token();
    let calendar = connect(&token)
        .calendar_create(&MsgraphCalendar {
            name: MsgraphField::Set(tag()),
            ..Default::default()
        })
        .expect("create the calendar")
        .response
        .id;

    with_cleanup(
        || {
            let created = graph(
                &token,
                "POST",
                &format!("calendars/{calendar}/events"),
                Some(serde_json::json!({
                    "subject": format!("Live Teams {}", tag()),
                    "start": { "dateTime": "2026-11-10T09:00:00", "timeZone": "UTC" },
                    "end": { "dateTime": "2026-11-10T10:00:00", "timeZone": "UTC" },
                    "isOnlineMeeting": true,
                    "onlineMeetingProvider": "teamsForBusiness",
                })),
            );
            let Some(url) = created["onlineMeeting"]["joinUrl"].as_str() else {
                eprintln!("SKIPPED: Graph gave the meeting no join link, the tenant lacks Teams");
                return;
            };

            let extra = user_line("msgraph-calendar");
            let a = Replica::open("msgraph-calendar", &extra, &token);
            a.sync(&calendar);
            let items = a.items(&calendar, "");
            assert_eq!(items.len(), 1, "{items:?}");
            let body = a.body(&items[0]).replace("\r\n ", "");
            assert!(
                body.contains(&format!("CONFERENCE;VALUE=URI;FEATURE=AUDIO,VIDEO:{url}")),
                "{body}"
            );
        },
        || {
            if let Err(err) = connect(&token).calendar_delete(&calendar) {
                eprintln!("WARNING: leftover calendar {calendar}: {err:?}");
            }
        },
    );
}

/// An event added through the store with `X-PIMDIR-ONLINE-MEETING:TRUE` is
/// created as an online meeting, and the store reads it back without the
/// request property (pimdir STORAGE Annex B.1).
///
/// A tenant without Teams gives the meeting no link: the CONFERENCE check
/// is then skipped (the Pimalaya test tenant, 2026-10-04).
#[test]
#[ignore = "live: needs the app registration's client secret"]
fn a_graph_online_meeting_is_created_with_its_event() {
    let token = msgraph_token();
    let uid = tag();
    let calendar = connect(&token)
        .calendar_create(&MsgraphCalendar {
            name: MsgraphField::Set(tag()),
            ..Default::default()
        })
        .expect("create the calendar")
        .response
        .id;

    with_cleanup(
        || {
            let extra = user_line("msgraph-calendar");
            let a = Replica::open("msgraph-calendar", &extra, &token);
            a.sync(&calendar);

            let user = msgraph_user();
            let event = meeting(&uid, "online", &user, &user)
                .replace("END:VEVENT", "X-PIMDIR-ONLINE-MEETING:TRUE\r\nEND:VEVENT");
            a.add(&calendar, &event, &[]);
            a.sync_until(&calendar, "the event is read back", |a| {
                a.item(&calendar, &uid)
                    .is_some_and(|item| !a.body(&item).contains("X-PIMDIR-ONLINE-MEETING"))
            });

            let path = format!("calendars/{calendar}/events?$select=isOnlineMeeting,onlineMeeting");
            let created = graph(&token, "GET", &path, None)["value"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            assert_eq!(created.len(), 1, "{created:?}");
            assert_eq!(created[0]["isOnlineMeeting"], true, "{created:?}");

            let Some(url) = created[0]["onlineMeeting"]["joinUrl"].as_str() else {
                eprintln!("SKIPPED: Graph gave the meeting no join link, the tenant lacks Teams");
                return;
            };
            let body = a
                .body(&a.item(&calendar, &uid).unwrap())
                .replace("\r\n ", "");
            assert!(
                body.contains(&format!("CONFERENCE;VALUE=URI;FEATURE=AUDIO,VIDEO:{url}")),
                "{body}"
            );
        },
        || {
            if let Err(err) = connect(&token).calendar_delete(&calendar) {
                eprintln!("WARNING: leftover calendar {calendar}: {err:?}");
            }
            delete_messages_about(&token, &uid);
        },
    );
}

/// A one-hour event `marker`, organised by `organizer` and attended by
/// `attendee`.
fn meeting(uid: &str, marker: &str, organizer: &str, attendee: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\n\
         VERSION:2.0\r\n\
         PRODID:-//pimalaya//neverest live tests//EN\r\n\
         BEGIN:VEVENT\r\n\
         UID:{uid}\r\n\
         DTSTAMP:20260101T000000Z\r\n\
         DTSTART:20261110T090000Z\r\n\
         DTEND:20261110T100000Z\r\n\
         SUMMARY:Live {marker} {uid}\r\n\
         ORGANIZER:mailto:{organizer}\r\n\
         ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{attendee}\r\n\
         END:VEVENT\r\n\
         END:VCALENDAR\r\n",
    )
}

/// An address of the test tenant that holds no mailbox, so whatever is
/// sent there bounces back inside the tenant.
fn tenant_stranger(tag: &str) -> String {
    let domain = msgraph_user()
        .rsplit_once('@')
        .map(|(_, domain)| domain.to_owned())
        .expect("the test mailbox is an address");
    format!("{tag}@{domain}")
}

/// Runs one raw Graph request against the test mailbox.
fn graph(
    token: &str,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> serde_json::Value {
    use io_msgraph::v1::send::{MSGRAPH_API_BASE, MsgraphSend, user_path};
    use url::Url;

    let mut client = connect(token);
    let url = Url::parse(MSGRAPH_API_BASE)
        .unwrap()
        .join(&format!("{}/{path}", user_path(&msgraph_user())))
        .unwrap();
    let body = body
        .map(|body| body.to_string().into_bytes())
        .unwrap_or_default();
    let content = (!body.is_empty()).then_some("application/json");
    let send = MsgraphSend::<serde_json::Value>::with_method(
        &client.auth.clone(),
        method,
        url,
        content,
        body,
    );
    client.run(send).expect("run the Graph request").response
}

/// The events of the mailbox holding `uid`.
fn events_with_uid(token: &str, uid: &str) -> Vec<serde_json::Value> {
    let path = format!(
        "events?$filter=iCalUId%20eq%20'{uid}'&$select=id,isOrganizer,responseStatus,isCancelled"
    );
    graph(token, "GET", &path, None)["value"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// A meeting added through the store invites its attendee, a reply to it is
/// refused since the account organises it, and a cancel removes it.
#[test]
#[ignore = "live: needs the app registration's client secret"]
fn a_graph_meeting_is_cancelled() {
    let token = msgraph_token();
    let uid = tag();
    let calendar = connect(&token)
        .calendar_create(&MsgraphCalendar {
            name: MsgraphField::Set(tag()),
            ..Default::default()
        })
        .expect("create the calendar")
        .response
        .id;

    with_cleanup(
        || {
            let extra = user_line("msgraph-calendar");
            let a = Replica::open("msgraph-calendar", &extra, &token);
            a.sync(&calendar);

            let attendee = tenant_stranger(&uid);
            a.add(
                &calendar,
                &meeting(&uid, "meeting", &msgraph_user(), &attendee),
                &[],
            );
            a.sync_until(&calendar, "the meeting is bound", |a| {
                a.item(&calendar, &uid).is_some()
            });
            let seq = a.item(&calendar, &uid).unwrap().seq;

            a.intent(
                &calendar,
                "calendar-reply",
                serde_json::json!({ "v": 1, "source": "msgraph-calendar", "seq": seq, "partstat": "DECLINED" }),
            );
            let intents = a.sync_intents(&calendar);
            assert_eq!(intents.len(), 1, "{intents:?}");
            assert_eq!(
                intents[0]["parked"], true,
                "Graph refuses the organizer's reply"
            );

            a.intent(
                &calendar,
                "calendar-cancel",
                serde_json::json!({
                    "v": 1,
                    "source": "msgraph-calendar",
                    "seq": seq,
                    "comment": "Cancelled by the neverest live tests",
                }),
            );
            let intents = a.sync_intents(&calendar);
            assert_eq!(intents.len(), 1, "{intents:?}");
            assert!(intents[0]["error"].is_null(), "{intents:?}");
            assert_eq!(a.queue().0, 0, "the cancel is acknowledged");

            a.sync_until(&calendar, "the cancelled meeting leaves the store", |a| {
                a.item(&calendar, &uid).is_none()
            });
        },
        || {
            if let Err(err) = connect(&token).calendar_delete(&calendar) {
                eprintln!("WARNING: leftover calendar {calendar}: {err:?}");
            }
            delete_messages_about(&token, &uid);
        },
    );
}

/// A meeting edited right after its creation reaches Graph, and its
/// cancellation stays one.
///
/// Exchange restamps a meeting once it has sent the invitations, so the
/// first push of the edit parks and the next run merges it. The edit is the
/// one a calendar client makes, a new `DTSTAMP` and a raised `SEQUENCE`,
/// which used to collide with Graph's own stamps: the item parked for good,
/// and the cancellation was undone by the edit still pending.
#[test]
#[ignore = "live: needs the app registration's client secret"]
fn a_graph_meeting_edited_right_after_creation_is_pushed_then_cancelled() {
    let token = msgraph_token();
    let uid = tag();
    let calendar = connect(&token)
        .calendar_create(&MsgraphCalendar {
            name: MsgraphField::Set(tag()),
            ..Default::default()
        })
        .expect("create the calendar")
        .response
        .id;

    with_cleanup(
        || {
            let extra = user_line("msgraph-calendar");
            let a = Replica::open("msgraph-calendar", &extra, &token);
            a.sync(&calendar);

            let attendee = tenant_stranger(&uid);
            a.add(
                &calendar,
                &meeting(&uid, "meeting", &msgraph_user(), &attendee),
                &[],
            );
            a.sync_until(&calendar, "the meeting is bound", |a| {
                a.item(&calendar, &uid).is_some()
            });

            let item = a.item(&calendar, &uid).unwrap();
            let edited = a
                .body(&item)
                .replace("SUMMARY:Live meeting", "SUMMARY:Live edited")
                .replace("BEGIN:VEVENT\r\n", "BEGIN:VEVENT\r\nSEQUENCE:1\r\n");
            let edited = edited
                .lines()
                .map(|line| match line.starts_with("DTSTAMP:") {
                    true => "DTSTAMP:20261004T120000Z",
                    false => line,
                })
                .collect::<Vec<_>>()
                .join("\r\n")
                + "\r\n";
            a.update(&calendar, item.seq, &edited);

            // NOTE: Graph mints its own iCalUId, so the calendar is listed.
            let subject = |token: &str| {
                let path = format!("calendars/{calendar}/events?$select=subject");
                graph(token, "GET", &path, None)["value"][0]["subject"]
                    .as_str()
                    .map(str::to_owned)
            };
            a.sync_until(&calendar, "the edit reaches Graph", |_| {
                subject(&token).is_some_and(|subject| subject.contains("Live edited"))
            });

            let seq = a.item(&calendar, &uid).unwrap().seq;
            a.intent(
                &calendar,
                "calendar-cancel",
                serde_json::json!({
                    "v": 1,
                    "source": "msgraph-calendar",
                    "seq": seq,
                    "comment": "Cancelled by the neverest live tests",
                }),
            );
            let intents = a.sync_intents(&calendar);
            assert!(intents[0]["error"].is_null(), "{intents:?}");
            a.sync_until(&calendar, "the cancelled meeting leaves the store", |a| {
                a.item(&calendar, &uid).is_none()
            });

            // NOTE: two more runs, so a pending edit would have had its chance
            // to bring the meeting back.
            a.sync(&calendar);
            a.sync(&calendar);
            let path = format!("calendars/{calendar}/events?$select=subject,isCancelled");
            let live: Vec<_> = graph(&token, "GET", &path, None)["value"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|event| event["isCancelled"] != true)
                .collect();
            assert!(live.is_empty(), "the cancelled meeting came back: {live:?}");
            assert!(a.item(&calendar, &uid).is_none());
        },
        || {
            if let Err(err) = connect(&token).calendar_delete(&calendar) {
                eprintln!("WARNING: leftover calendar {calendar}: {err:?}");
            }
            delete_messages_about(&token, &uid);
        },
    );
}

/// An invitation received by mail is answered: Graph accepts it and the
/// answer is on the event.
///
/// The test tenant holds one mailbox, so the invitation is an iMIP message
/// it sends itself, organised by an address of the tenant holding none.
#[test]
#[ignore = "live: needs the app registration's client secret"]
fn a_graph_invitation_is_answered() {
    use std::{thread, time::Duration};

    let token = msgraph_token();
    let uid = tag();
    let me = msgraph_user();
    let organizer = tenant_stranger(&uid);
    let ics = meeting(&uid, "invitation", &organizer, &me)
        .replace("VERSION:2.0\r\n", "VERSION:2.0\r\nMETHOD:REQUEST\r\n");
    let mime = format!(
        "From: {me}\r\n\
         To: {me}\r\n\
         Subject: {uid}\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: text/calendar; charset=utf-8; method=REQUEST\r\n\
         \r\n\
         {ics}"
    );

    with_cleanup(
        || {
            connect(&token)
                .mail_send_mime(mime.as_bytes())
                .expect("send the invitation");

            let mut found = Vec::new();
            for _ in 0..30 {
                found = events_with_uid(&token, &uid);
                if !found.is_empty() {
                    break;
                }
                thread::sleep(Duration::from_secs(3));
            }
            assert_eq!(found.len(), 1, "Exchange files the invitation once");
            assert_eq!(found[0]["isOrganizer"], false);

            let calendar = graph(&token, "GET", "calendar?$select=id", None)["id"]
                .as_str()
                .expect("the default calendar has an id")
                .to_owned();
            let extra = user_line("msgraph-calendar");
            let a = Replica::open("msgraph-calendar", &extra, &token);
            a.sync_until(&calendar, "the invitation reaches the store", |a| {
                a.item(&calendar, &uid).is_some()
            });
            let seq = a.item(&calendar, &uid).unwrap().seq;

            a.intent(
                &calendar,
                "calendar-reply",
                serde_json::json!({
                    "v": 1,
                    "source": "msgraph-calendar",
                    "seq": seq,
                    "partstat": "ACCEPTED",
                    "comment": "Sent by the neverest live tests",
                }),
            );
            let intents = a.sync_intents(&calendar);
            assert_eq!(intents.len(), 1, "{intents:?}");
            assert!(intents[0]["error"].is_null(), "{intents:?}");
            assert_eq!(a.queue(), (0, Vec::new()), "the reply is acknowledged");

            let answered = events_with_uid(&token, &uid);
            assert_eq!(answered[0]["responseStatus"]["response"], "accepted");
        },
        || {
            for event in events_with_uid(&token, &uid) {
                let Some(id) = event["id"].as_str() else {
                    continue;
                };
                if let Err(err) = connect(&token).event_delete(id) {
                    eprintln!("WARNING: leftover invitation {id}: {err:?}");
                }
            }
            // NOTE: the invitation, its sent copy, the reply and the
            // bounces of the organizer that holds no mailbox.
            delete_messages_about(&token, &uid);
        },
    );
}

/// Deletes the messages a run's invitations left in the test mailbox: the
/// sent copies, the requests, the replies and the bounces naming `uid`.
///
/// A bounce and the search index both lag behind the send, so the search
/// runs a few times, some seconds apart.
fn delete_messages_about(token: &str, uid: &str) {
    use std::{thread, time::Duration};

    let path = format!("messages?$search=%22{uid}%22&$select=id&$top=100");
    for _ in 0..3 {
        thread::sleep(Duration::from_secs(10));
        let messages = graph(token, "GET", &path, None)["value"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for message in messages {
            let Some(id) = message["id"].as_str() else {
                continue;
            };
            if let Err(err) = connect(token).message_delete(id) {
                eprintln!("WARNING: leftover message {id}: {err:?}");
            }
        }
    }
}
