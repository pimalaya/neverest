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
