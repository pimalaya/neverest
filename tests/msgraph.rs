//! Live plumbing tests against Microsoft Graph: mail, contacts and calendars,
//! on the Pimalaya test mailbox. Ignored by default.
//!
//! As in tests/google.rs, each runs two replicas of one account against one
//! throwaway collection (see tests/common). Graph mail is pull-only for new
//! messages, so the mail test seeds its message through the API and crosses
//! flags and a removal only.
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

use io_msgraph::v1::{
    client::{MsgraphClientStd, MsgraphClientStdConnectOptions},
    field::MsgraphField,
    rest::users::{
        calendars::MsgraphCalendar, contact_folders::MsgraphContactFolder,
        mail_folders::MsgraphMailFolder,
    },
};

use crate::common::*;

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
            // NOTE: Exchange files a mailed invitation under its own global
            // object id, which wraps the UID in hex (`vCal-Uid`).
            let key: String = uid.bytes().map(|byte| format!("{byte:02X}")).collect();
            let extra = user_line("msgraph-calendar");
            let a = Replica::open("msgraph-calendar", &extra, &token);
            a.sync_until(&calendar, "the invitation reaches the store", |a| {
                a.item(&calendar, &key).is_some()
            });
            let seq = a.item(&calendar, &key).unwrap().seq;

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
