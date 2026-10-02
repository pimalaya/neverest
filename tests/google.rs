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
