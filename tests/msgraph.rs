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
