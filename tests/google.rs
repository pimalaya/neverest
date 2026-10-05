//! Live plumbing tests against Google: Gmail, People and Calendar, on the
//! Pimalaya Workspace user. Ignored by default.
//!
//! The protocol crates are tested live on their own and the sync on local
//! servers, so these prove the adapters in between: each runs two replicas
//! of one account against one throwaway collection (see tests/common), one
//! pushing what the other pulls through its delta round.
//!
//! Every resource a run creates is named `neverest-live-<millis>` and deleted
//! however the run ends. Run with a service account key holding domain-wide
//! delegation for the three scopes below:
//!
//! ```sh
//! GOOGLE_SERVICE_ACCOUNT_KEY_FILE=key.json \
//! cargo test --test google -- --ignored --test-threads=1
//! ```

#![cfg(any(feature = "gmail", feature = "gpeople", feature = "gcal"))]

mod common;

#[cfg(feature = "gcal")]
use io_gcal::v3::{
    client::{GcalClientStd, GcalClientStdConnectOptions},
    rest::calendars::GcalCalendar,
};
#[cfg(feature = "gmail")]
use io_gmail::v1::{
    client::{GmailClientStd, GmailClientStdConnectOptions},
    rest::{labels::GmailLabel, messages::list::GmailMessagesListParams},
};
#[cfg(feature = "gpeople")]
use io_gpeople::v1::{
    client::{GpeopleClientStd, GpeopleClientStdConnectOptions},
    rest::people::{GpeoplePersonField, GpeopleReadSourceType},
};
#[cfg(feature = "gpeople")]
use pimalaya_stream::{proxy::Proxy, tls::Tls};

use crate::common::*;

#[cfg(feature = "gmail")]
const GMAIL_SCOPE: &str = "https://mail.google.com/";
#[cfg(feature = "gpeople")]
const CONTACTS_SCOPE: &str = "https://www.googleapis.com/auth/contacts";
#[cfg(feature = "gcal")]
const CALENDAR_SCOPE: &str = "https://www.googleapis.com/auth/calendar";

/// A message added to a label crosses, its flags cross back, a fresh replica
/// reads both, and a removal archives it rather than deleting it.
#[test]
#[ignore = "live: needs a Google service account key"]
#[cfg(feature = "gmail")]
fn a_gmail_label_syncs_both_ways() {
    let token = google_token(GMAIL_SCOPE);
    let connect = || {
        GmailClientStd::connect(&token, GmailClientStdConnectOptions::default())
            .expect("connect to Gmail")
    };
    let label = tag();
    let label_id = connect()
        .label_create(&GmailLabel {
            name: label.clone(),
            ..Default::default()
        })
        .expect("create the label")
        .response
        .id;

    // Every copy of the run's message, whichever labels it kept.
    let messages = || {
        let query = format!("rfc822msgid:{label}@pimalaya.org");
        let params = GmailMessagesListParams {
            q: Some(&query),
            include_spam_trash: true,
            ..Default::default()
        };
        connect()
            .messages_list(&params)
            .expect("list the run's messages")
            .response
            .messages
    };

    with_cleanup(
        || {
            let a = Replica::open("gmail", "", &token);
            let b = Replica::open("gmail", "", &token);

            a.sync(&label);
            b.sync(&label);

            a.add(&label, &message(&label, "google@pimalaya.org"), &["\\Seen"]);
            a.sync(&label);

            b.sync_until(&label, "the message reaches the other replica", |b| {
                b.item(&label, &label)
                    .is_some_and(|item| flags(&item).contains("\\Seen"))
            });

            let item = b.item(&label, &label).unwrap();
            let mut set = flags(&item);
            set.insert(String::from("\\Flagged"));
            let set: Vec<&str> = set.iter().map(String::as_str).collect();
            b.set_flags(&label, item.seq, &set);
            b.sync(&label);

            a.sync_until(&label, "the star reaches the first replica", |a| {
                a.item(&label, &label)
                    .is_some_and(|item| flags(&item).contains("\\Flagged"))
            });

            let c = Replica::open("gmail", "", &token);
            c.sync(&label);
            let item = c.item(&label, &label).expect("a full round reads it");
            assert!(flags(&item).contains("\\Flagged"), "with its star");
            assert!(
                c.body(&item).contains("Sent by the neverest live tests."),
                "and its body",
            );

            let seq = a.item(&label, &label).unwrap().seq;
            a.remove(&label, seq);
            a.sync(&label);

            b.sync_until(&label, "the removal reaches the other replica", |b| {
                b.item(&label, &label).is_none()
            });
            assert_eq!(
                messages().len(),
                1,
                "a removal drops the label and keeps the message",
            );
        },
        || {
            let mut client = connect();
            for message in messages() {
                if let Err(err) = client.message_delete(&message.id) {
                    eprintln!("WARNING: leftover message {}: {err:?}", message.id);
                }
            }
            if let Err(err) = client.label_delete(&label_id) {
                eprintln!("WARNING: leftover label {label}: {err:?}");
            }
        },
    );
}

/// A card crosses both ways through the one People address book.
#[test]
#[ignore = "live: needs a Google service account key"]
#[cfg(feature = "gpeople")]
fn google_contacts_sync_both_ways() {
    let token = google_token(CONTACTS_SCOPE);

    with_cleanup(
        || round_trip("gpeople", "", &token, "contacts", card),
        || {
            // NOTE: only a failed run leaves a card behind, and the
            // search index may lag behind it.
            let mut client = GpeopleClientStd::connect(
                &token,
                GpeopleClientStdConnectOptions {
                    tls: Tls::default(),
                    proxy: Proxy::None,
                },
            )
            .expect("connect to People");
            let found = client
                .contacts_search(
                    "neverest-live",
                    &[GpeoplePersonField::Names],
                    None,
                    &[GpeopleReadSourceType::ReadSourceTypeContact],
                )
                .expect("search the run's cards")
                .response;
            for person in found.results.into_iter().filter_map(|r| r.person) {
                if let Err(err) = client.contact_delete(&person.resource_name) {
                    eprintln!("WARNING: leftover card {}: {err:?}", person.resource_name);
                }
            }
        },
    );
}

/// An event crosses both ways through a throwaway calendar.
#[test]
#[ignore = "live: needs a Google service account key"]
#[cfg(feature = "gcal")]
fn a_google_calendar_syncs_both_ways() {
    let token = google_token(CALENDAR_SCOPE);
    let connect =
        || GcalClientStd::connect(&token, GcalClientStdConnectOptions::default()).unwrap();
    let calendar = connect()
        .calendar_insert(&GcalCalendar {
            summary: Some(tag()),
            ..Default::default()
        })
        .expect("create the calendar")
        .response
        .id
        .expect("the calendar has an id");

    with_cleanup(
        || round_trip("gcal", "", &token, &calendar, event),
        || {
            if let Err(err) = connect().calendar_delete(&calendar) {
                eprintln!("WARNING: leftover calendar {calendar}: {err:?}");
            }
        },
    );
}

/// The Workspace user the invitations go to: the delegated subject itself,
/// so nothing leaves the Pimalaya domain.
#[cfg(feature = "gcal")]
fn google_subject() -> String {
    std::env::var("GOOGLE_SERVICE_ACCOUNT_SUBJECT")
        .unwrap_or_else(|_| String::from("google@pimalaya.org"))
}

/// A one-hour event `marker`, organised by `organizer` and attended by
/// `attendee`, both left to the server's own scheduling.
#[cfg(feature = "gcal")]
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

/// A throwaway calendar, deleted with every copy of the run's events
/// however the run ends.
#[cfg(feature = "gcal")]
fn with_google_calendar(token: &str, uid: &str, body: impl FnOnce(&str)) {
    use io_gcal::v3::rest::events::list::GcalEventsListParams;

    let connect = || GcalClientStd::connect(token, GcalClientStdConnectOptions::default()).unwrap();
    let calendar = connect()
        .calendar_insert(&GcalCalendar {
            summary: Some(tag()),
            ..Default::default()
        })
        .expect("create the calendar")
        .response
        .id
        .expect("the calendar has an id");

    with_cleanup(
        || body(&calendar),
        || {
            let mut client = connect();
            // NOTE: the invitation's copy on the attendee's own calendar.
            let params = GcalEventsListParams {
                ical_uid: Some(uid),
                ..Default::default()
            };
            if let Ok(copies) = client.events_list("primary", &params) {
                for event in copies.response.items {
                    let Some(id) = event.id else { continue };
                    if let Err(err) = client.event_delete("primary", &id, None, None) {
                        eprintln!("WARNING: leftover invitation {id}: {err:?}");
                    }
                }
            }
            if let Err(err) = client.calendar_delete(&calendar) {
                eprintln!("WARNING: leftover calendar {calendar}: {err:?}");
            }
            #[cfg(feature = "gmail")]
            delete_mail_about(uid);
        },
    );
}

/// Deletes the notices a run's invitations left in the Workspace user's
/// mailbox: the invitations, the replies and the cancellations naming `uid`.
///
/// Delivery lags behind the calendar, so the search runs a few times, some
/// seconds apart.
#[cfg(all(feature = "gcal", feature = "gmail"))]
fn delete_mail_about(uid: &str) {
    use std::{thread, time::Duration};

    let token = google_token(GMAIL_SCOPE);
    let mut client = GmailClientStd::connect(&token, GmailClientStdConnectOptions::default())
        .expect("connect to Gmail");
    for _ in 0..3 {
        thread::sleep(Duration::from_secs(10));
        let params = GmailMessagesListParams {
            q: Some(uid),
            include_spam_trash: true,
            ..Default::default()
        };
        let Ok(found) = client.messages_list(&params) else {
            continue;
        };
        for message in found.response.messages {
            if let Err(err) = client.message_delete(&message.id) {
                eprintln!("WARNING: leftover notice {}: {err:?}", message.id);
            }
        }
    }
}

/// A meeting added through the store keeps its UID and invites its
/// attendee, a reply to it is refused since the account organises it, and a
/// cancel removes it.
#[test]
#[ignore = "live: needs a Google service account key"]
#[cfg(feature = "gcal")]
fn a_google_meeting_is_invited_to_and_cancelled() {
    use io_gcal::v3::rest::events::list::GcalEventsListParams;

    let token = google_token(CALENDAR_SCOPE);
    let uid = tag();
    let subject = google_subject();

    with_google_calendar(&token, &uid, |calendar| {
        let a = Replica::open("gcal", "", &token);
        a.sync(calendar);

        a.add(calendar, &meeting(&uid, "meeting", &subject, &subject), &[]);
        a.sync(calendar);

        let mut client = GcalClientStd::connect(&token, GcalClientStdConnectOptions::default())
            .expect("connect to Calendar");
        let params = GcalEventsListParams {
            ical_uid: Some(&uid),
            ..Default::default()
        };
        let created = client
            .events_list(calendar, &params)
            .expect("list the meeting")
            .response
            .items;
        assert_eq!(created.len(), 1, "one event holds the UID");
        let event = &created[0];
        assert_eq!(event.ical_uid.as_deref(), Some(uid.as_str()));
        assert_eq!(
            event.organizer.as_ref().and_then(|o| o.is_self),
            Some(true),
            "inserted, so Google organises it and invites, not imported",
        );
        assert!(
            event
                .attendees
                .iter()
                .any(|attendee| attendee.email.as_deref() == Some(subject.as_str())),
            "with its attendee",
        );

        a.sync_until(calendar, "the meeting is bound", |a| {
            a.item(calendar, &uid).is_some()
        });
        let seq = a.item(calendar, &uid).unwrap().seq;

        a.intent(
            calendar,
            "calendar-reply",
            serde_json::json!({ "v": 1, "source": "gcal", "seq": seq, "partstat": "ACCEPTED" }),
        );
        let intents = a.sync_intents(calendar);
        assert_eq!(intents.len(), 1, "{intents:?}");
        assert_eq!(
            intents[0]["parked"], true,
            "the organizer has nothing to reply to"
        );

        a.intent(
            calendar,
            "calendar-cancel",
            serde_json::json!({ "v": 1, "source": "gcal", "seq": seq, "comment": "Cancelled by the neverest live tests" }),
        );
        let intents = a.sync_intents(calendar);
        assert_eq!(intents.len(), 1, "{intents:?}");
        assert!(intents[0]["error"].is_null(), "{intents:?}");
        assert_eq!(a.queue().0, 0, "the cancel is acknowledged");

        a.sync_until(calendar, "the cancelled meeting leaves the store", |a| {
            a.item(calendar, &uid).is_none()
        });
    });
}

/// An event added through the store with `X-PIMDIR-ONLINE-MEETING:TRUE`
/// gets a Google Meet: the store reads it back as a `CONFERENCE`, the
/// request property gone (pimdir STORAGE Annex B.1).
#[test]
#[ignore = "live: needs a Google service account key"]
#[cfg(feature = "gcal")]
fn a_google_meet_is_created_with_its_event() {
    use io_gcal::v3::rest::events::list::GcalEventsListParams;

    let token = google_token(CALENDAR_SCOPE);
    let uid = tag();
    let subject = google_subject();

    with_google_calendar(&token, &uid, |calendar| {
        let a = Replica::open("gcal", "", &token);
        a.sync(calendar);

        let event = meeting(&uid, "meet", &subject, &subject)
            .replace("END:VEVENT", "X-PIMDIR-ONLINE-MEETING:TRUE\r\nEND:VEVENT");
        a.add(calendar, &event, &[]);
        a.sync(calendar);

        let mut client = GcalClientStd::connect(&token, GcalClientStdConnectOptions::default())
            .expect("connect to Calendar");
        let params = GcalEventsListParams {
            ical_uid: Some(&uid),
            ..Default::default()
        };
        let created = client
            .events_list(calendar, &params)
            .expect("list the meeting")
            .response
            .items;
        assert_eq!(created.len(), 1, "one event holds the UID");
        let conference = created[0]
            .conference_data
            .as_ref()
            .expect("Google attached a conference");
        assert!(
            conference.create_request.is_some() || !conference.entry_points.is_empty(),
            "{conference:?}"
        );

        a.sync_until(calendar, "the Meet link reaches the store", |a| {
            a.item(calendar, &uid)
                .map(|item| a.body(&item).replace("\r\n ", ""))
                .is_some_and(|body| body.contains("CONFERENCE;VALUE=URI;FEATURE=AUDIO,VIDEO"))
        });
        let body = a.body(&a.item(calendar, &uid).unwrap());
        assert!(!body.contains("X-PIMDIR-ONLINE-MEETING"), "{body}");
    });
}

/// An invitation the calendar attends is answered: the reply reaches
/// Google and its `PARTSTAT` comes back with the next sync.
#[test]
#[ignore = "live: needs a Google service account key"]
#[cfg(feature = "gcal")]
fn a_google_invitation_is_answered() {
    use io_gcal::v3::rest::events::{GcalEvent, import::GcalEventImportParams};

    let token = google_token(CALENDAR_SCOPE);
    let uid = tag();
    let subject = google_subject();

    with_google_calendar(&token, &uid, |calendar| {
        // NOTE: the organizer is the Workspace user and the attendee the
        // throwaway calendar, so the reply stays in the domain.
        let invitation = meeting(&uid, "invitation", &subject, calendar);
        let event = GcalEvent::from_ical(invitation.as_bytes()).expect("project the invitation");
        GcalClientStd::connect(&token, GcalClientStdConnectOptions::default())
            .expect("connect to Calendar")
            .event_import(calendar, &event, &GcalEventImportParams::default())
            .expect("import the invitation");

        let a = Replica::open("gcal", "", &token);
        a.sync_until(calendar, "the invitation reaches the store", |a| {
            a.item(calendar, &uid).is_some()
        });
        let seq = a.item(calendar, &uid).unwrap().seq;

        a.intent(
            calendar,
            "calendar-reply",
            serde_json::json!({
                "v": 1,
                "source": "gcal",
                "seq": seq,
                "partstat": "TENTATIVE",
                "comment": "Sent by the neverest live tests",
            }),
        );
        let intents = a.sync_intents(calendar);
        assert_eq!(intents.len(), 1, "{intents:?}");
        assert!(intents[0]["error"].is_null(), "{intents:?}");
        assert_eq!(a.queue(), (0, Vec::new()), "the reply is acknowledged");

        a.sync_until(calendar, "the answer comes back", |a| {
            a.item(calendar, &uid)
                .is_some_and(|item| a.body(&item).contains("PARTSTAT=TENTATIVE"))
        });
    });
}
