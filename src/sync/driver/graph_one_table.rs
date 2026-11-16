//! # One Graph table
//!
//! The one `msgraph` table over a store the three Graph tables synced (cairn
//! change graph-domains-share-one-table).
//!
//! Each domain opens the source id holding its bindings, so mail and contacts
//! resume from their delta links and the calendar finds the event it bound,
//! nothing created, read again or listed whole.

use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, atomic::AtomicU64},
};

use io_pimdir::{client::reader::PimdirReader, collection::PimdirScope};
use pimalaya_stream::tls::Tls;
use secrecy::SecretString;
use serde_json::json;

use super::{Authority, Noted, SourceCtx, open_store, projection_view, sync_side};
use crate::{
    account::{MsgraphAccount, SourceAccount, SourceAccountBackend},
    client::{Client, Pool},
    config::{
        AccountConfig,
        permissions::{
            CollectionPermissions, FlagSourcePermissions, ItemSourcePermissions, SourcePermissions,
        },
        source::SourceBackendConfig,
    },
    msgraph::client::{GraphClient, GraphKind, SizeHints},
    scripted::Step,
    throttle::Throttle,
};

const GRAPH: &str = "https://graph.microsoft.com/v1.0/me";

/// One Graph domain as a store holds it, and what its server answers
/// each run.
struct Domain {
    kind: GraphKind,
    media: &'static str,
    /// The collection's name on the wire.
    wire: &'static str,
    first: Vec<Step>,
    second: Vec<Step>,
    /// What the second run's one request names, its stored delta link.
    resumes: &'static str,
    /// The handle the collection binds.
    handle: &'static str,
}

/// A lone event, its UID in the projection's stash.
fn lone() -> serde_json::Value {
    json!({
        "id": "e1",
        "changeKey": "k1",
        "type": "singleInstance",
        "subject": "Lunch",
        "start": { "dateTime": "2026-11-23T12:00:00.0000000", "timeZone": "UTC" },
        "end": { "dateTime": "2026-11-23T13:00:00.0000000", "timeZone": "UTC" },
        "singleValueExtendedProperties": [{
            "id": "String {c8e5e5cf-3f6c-4f0a-9d4e-52f1e7b2a9d3} Name pimalaya-ical",
            "value": "UID:e1@example.org",
        }],
    })
}

/// A first round landing one item and leaving a delta link where the domain
/// has one, then a run with nothing new.
fn domain(kind: GraphKind) -> Domain {
    let delta = |path: &str, token: &str| format!("{GRAPH}/{path}/delta?$deltatoken={token}");
    match kind {
        GraphKind::Mail => {
            let path = "mailFolders/id-inbox/messages";
            let message = json!({
                "id": "m1",
                "subject": "Subject m1",
                "sentDateTime": "2026-10-06T08:00:00Z",
                "receivedDateTime": "2026-10-06T08:00:01Z",
                "isRead": false,
                "isDraft": false,
                "flag": { "flagStatus": "notFlagged" },
                "hasAttachments": false,
                "internetMessageId": "<m1@example.org>",
            });
            Domain {
                kind,
                media: "message/rfc822",
                wire: "Inbox",
                first: vec![Step::ok(json!({
                    "value": [message],
                    "@odata.deltaLink": delta(path, "d1"),
                }))],
                second: vec![Step::ok(json!({
                    "value": [],
                    "@odata.deltaLink": delta(path, "d2"),
                }))],
                resumes: "deltatoken=d1",
                handle: "m1",
            }
        }
        GraphKind::Contacts => Domain {
            kind,
            media: "text/vcard",
            wire: "contacts",
            first: vec![
                Step::ok(json!({
                    "value": [{ "id": "c1", "changeKey": "k1" }],
                    "@odata.deltaLink": delta("contacts", "t1"),
                })),
                Step::ok(json!({
                    "id": "c1",
                    "changeKey": "k1",
                    "displayName": "Ann Example",
                    "givenName": "Ann",
                    "surname": "Example",
                })),
            ],
            second: vec![Step::ok(json!({
                "value": [],
                "@odata.deltaLink": delta("contacts", "t2"),
            }))],
            resumes: "deltatoken=t1",
            handle: "c1",
        },
        GraphKind::Calendar => Domain {
            kind,
            media: "text/calendar",
            wire: "AAMk",
            first: vec![Step::ok(json!({ "value": [lone()] })), Step::ok(lone())],
            second: vec![Step::ok(json!({ "value": [lone()] }))],
            resumes: "/calendars/AAMk/events",
            handle: "e1",
        },
    }
}

fn context(id: &str, kind: GraphKind, perms: SourcePermissions, client: GraphClient) -> SourceCtx {
    let account = SourceAccount {
        backend: SourceAccountBackend::Msgraph(MsgraphAccount {
            kind,
            token: SecretString::from(String::from("token")),
            user_id: String::from("me"),
            tls: Tls::default(),
            sizes: SizeHints::default(),
            proxy: pimalaya_stream::proxy::Proxy::None,
        }),
        #[cfg(feature = "smtp")]
        smtp: None,
        throttle: Arc::new(Throttle::new("msgraph")),
        downloaded: Arc::new(AtomicU64::new(0)),
    };
    SourceCtx {
        name: id.to_owned(),
        namespace: id.to_owned(),
        pool: Pool::of(account, Client::Msgraph(Box::new(client))),
        perms,
        authority: Authority::Shared,
        refused: Vec::new(),
        rejected: Vec::new(),
        scope: PimdirScope::unbounded(),
        dry_run: false,
        accept_absences: false,
        capped: false,
        noted: Noted::default(),
    }
}

/// Syncs the domain's collection under source `id` over `script`: the
/// handles it binds then and the request lines sent.
fn sync(
    dir: &Path,
    id: &str,
    perms: SourcePermissions,
    domain: &Domain,
    script: Vec<Step>,
) -> (BTreeSet<String>, Vec<String>) {
    let collection = format!("{id}/{}", domain.wire);
    let mut store = open_store(dir, id, "a").unwrap();
    let (client, server) =
        GraphClient::scripted_as(domain.kind, vec![script], &[("Inbox", "id-inbox")]);
    let mut ctx = context(id, domain.kind, perms, client);
    let blobs = store.blob_stage();
    sync_side(&collection, &mut ctx, &mut store, &blobs, false, None).expect("the sync");
    let handles = projection_view(&store, &collection, id)
        .unwrap()
        .into_iter()
        .map(|placement| placement.handle.0)
        .collect();
    let requests = server.lines();
    (handles, requests)
}

fn collections(dir: &Path) -> BTreeSet<String> {
    let reader = PimdirReader::open(dir).unwrap();
    let listed = reader.list_collections().unwrap();
    listed.into_iter().map(|collection| collection.id).collect()
}

/// The source ids today's three tables gave, pinned by
/// `config::tests::the_graph_tables_keep_their_source_ids`.
const THREE_TABLES: [(&str, GraphKind); 3] = [
    ("a/msgraph", GraphKind::Mail),
    ("a/msgraph-contacts", GraphKind::Contacts),
    ("a/msgraph-calendar", GraphKind::Calendar),
];

#[test]
fn the_one_graph_table_finds_what_the_three_tables_synced() {
    let dir = tempfile::tempdir().unwrap();
    let defaults = SourcePermissions {
        collection: CollectionPermissions::default(),
        flag: FlagSourcePermissions::default(),
        item: ItemSourcePermissions::default(),
    };

    for (id, kind) in THREE_TABLES {
        let domain = domain(kind);
        let collection = format!("{id}/{}", domain.wire);
        open_store(dir.path(), id, "a")
            .unwrap()
            .ensure_collection(&collection, domain.media)
            .unwrap();
        let (handles, _) = sync(dir.path(), id, defaults, &domain, domain.first.clone());
        assert_eq!(handles, BTreeSet::from([domain.handle.to_owned()]), "{id}");
    }
    let synced = collections(dir.path());
    assert_eq!(synced.len(), 3, "{synced:?}");

    let account: AccountConfig = toml::from_str(
        r#"
            msgraph.auth.token.raw = "token"
            msgraph.mail = true
            msgraph.contacts = true
            msgraph.calendar = true
            "#,
    )
    .unwrap();
    let sources = account.namespaced("a").unwrap().sources().unwrap();
    let ids: BTreeSet<&str> = sources.keys().map(String::as_str).collect();
    let three: BTreeSet<&str> = THREE_TABLES.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, three, "every source keeps its id");

    for (id, source) in &sources {
        let kind = match source.backend {
            SourceBackendConfig::Msgraph(_) => GraphKind::Mail,
            SourceBackendConfig::MsgraphContacts(_) => GraphKind::Contacts,
            SourceBackendConfig::MsgraphCalendar(_) => GraphKind::Calendar,
            _ => panic!("{id} is no Graph source"),
        };
        let domain = domain(kind);
        let (handles, requests) = sync(
            dir.path(),
            id,
            source.permissions(),
            &domain,
            domain.second.clone(),
        );
        assert_eq!(requests.len(), 1, "{id}: {requests:?}");
        assert!(requests[0].contains(domain.resumes), "{id}: {requests:?}");
        assert_eq!(handles, BTreeSet::from([domain.handle.to_owned()]), "{id}");
    }
    assert_eq!(collections(dir.path()), synced, "no collection created");
}
