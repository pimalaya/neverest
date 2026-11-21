//! # Clipped Graph series
//!
//! An open Graph series read from its window's floor, between a Graph
//! calendar and another endpoint of one store, through the driver over a
//! scripted Graph.
//!
//! An exception before the floor is unknown on Graph, not removed: the
//! other endpoint keeps it, while one inside the window Graph no longer
//! lists is deleted there as any edit.

use std::sync::{Arc, atomic::AtomicU64};

use io_pimdir::{
    change::{PimdirChange, PimdirChangeKind},
    client::{PimdirSourceStore, blobs::PimdirBlobStage},
    collection::{PimdirCollectionId, PimdirScope},
    placement::{PimdirFlags, PimdirHandle, PimdirLinkId, PimdirSortKey},
    remote::{
        PimdirEnumerate, PimdirEnumerated, PimdirFetchedBody, PimdirFetchedItem, PimdirPushOutcome,
        PimdirPushResult, PimdirRemote, PimdirRemoteItem, PimdirRemoteMeta, PimdirRemoteSnapshot,
        PimdirTier,
    },
};
use jiff::{Timestamp, ToSpan, tz::TimeZone};
use pimalaya_stream::tls::Tls;
use secrecy::SecretString;
use serde_json::json;

use super::{Authority, Noted, SourceCtx, open_store, reconcile::sync_verb, sync_side};
use crate::{
    account::{MsgraphAccount, SourceAccount, SourceAccountBackend},
    client::{Client, Pool},
    config::AccountConfig,
    msgraph::client::{GraphClient, GraphKind, SeriesClips, SizeHints},
    scripted::Step,
    sync::{run_verb, storage::load_side},
    throttle::Throttle,
};

const CAL: &str = "cal";

/// A yearly series with no end from 2000-03-12 at 09:00 UTC.
fn master(change_key: &str, subject: &str) -> serde_json::Value {
    json!({
        "id": "S",
        "changeKey": change_key,
        "type": "seriesMaster",
        "subject": subject,
        "start": { "dateTime": "2000-03-12T09:00:00.0000000", "timeZone": "UTC" },
        "end": { "dateTime": "2000-03-12T10:00:00.0000000", "timeZone": "UTC" },
        "recurrence": {
            "pattern": {
                "type": "absoluteYearly",
                "interval": 1,
                "month": 3,
                "dayOfMonth": 12,
            },
            "range": { "type": "noEnd", "startDate": "2000-03-12", "endDate": "0001-01-01" },
        },
        "singleValueExtendedProperties": [{
            "id": "String {c8e5e5cf-3f6c-4f0a-9d4e-52f1e7b2a9d3} Name pimalaya-ical",
            "value": "UID:birthday@example.org",
        }],
    })
}

/// The occurrence of `year` moved two hours later.
fn exception(year: i16) -> serde_json::Value {
    json!({
        "id": format!("X{year}"),
        "type": "exception",
        "seriesMasterId": "S",
        "originalStart": format!("{year}-03-12T09:00:00Z"),
        "subject": format!("Party {year}"),
        "start": { "dateTime": format!("{year}-03-12T11:00:00.0000000"), "timeZone": "UTC" },
        "end": { "dateTime": format!("{year}-03-12T12:00:00.0000000"), "timeZone": "UTC" },
    })
}

/// A run of the calendar: the listing, the master and its three windows,
/// the first answering `exceptions`.
fn run(change_key: &str, subject: &str, exceptions: Vec<serde_json::Value>) -> Vec<Step> {
    let master = master(change_key, subject);
    vec![
        Step::ok(json!({ "value": [master.clone()] })),
        Step::ok(master),
        Step::ok(json!({ "value": exceptions })),
        Step::ok(json!({ "value": [] })),
        Step::ok(json!({ "value": [] })),
    ]
}

fn context(client: GraphClient) -> SourceCtx {
    let config: AccountConfig =
        toml::from_str("msgraph.auth.token.raw = \"x\"\nmsgraph.calendar = true").unwrap();
    let perms = config.sources().unwrap()["msgraph-calendar"].permissions();
    let account = SourceAccount {
        backend: SourceAccountBackend::Msgraph(MsgraphAccount {
            kind: GraphKind::Calendar,
            token: SecretString::from(String::from("token")),
            user_id: String::from("me"),
            tls: Tls::default(),
            sizes: SizeHints::default(),
            clips: SeriesClips::default(),
            proxy: pimalaya_stream::proxy::Proxy::None,
        }),
        #[cfg(feature = "smtp")]
        smtp: None,
        throttle: Arc::new(Throttle::new("msgraph")),
        downloaded: Arc::new(AtomicU64::new(0)),
    };
    SourceCtx {
        name: String::from("graph"),
        namespace: String::from("graph"),
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

/// One pulling sync of the Graph side over `script`, answering the series
/// it noted clipped.
fn pull_graph(store: &mut PimdirSourceStore, script: Vec<Step>) -> Vec<(String, String, String)> {
    let (client, _server) = GraphClient::scripted_as(GraphKind::Calendar, vec![script], &[]);
    let mut ctx = context(client);
    let blobs = store.blob_stage();
    sync_side(CAL, &mut ctx, store, &blobs, false, None).expect("the Graph sync");
    match ctx.pool.primary() {
        Client::Msgraph(client) => client.clips.noted(),
        #[allow(unreachable_patterns)]
        _ => unreachable!(),
    }
}

/// The other endpoint, holding the series under `e1` at one revision and
/// recording the bodies it is sent.
struct Other {
    link_id: PimdirLinkId,
    body: Vec<u8>,
    blobs: PimdirBlobStage,
    sent: Vec<String>,
    removed: usize,
}

impl Other {
    fn item(&self) -> PimdirFetchedItem {
        PimdirFetchedItem {
            handle: PimdirHandle::from("e1"),
            link_id: self.link_id.clone(),
            summary: None,
            sort_key: PimdirSortKey::default(),
            body: Some(PimdirFetchedBody::Inline {
                hash: io_pimdir::hash::hash(&self.body),
                bytes: self.body.clone(),
            }),
            revision: Some(String::from("w1")),
        }
    }
}

impl PimdirRemote for Other {
    type Error = anyhow::Error;

    fn enumerate(
        &mut self,
        _collection: &PimdirCollectionId,
        _request: PimdirEnumerate,
    ) -> Result<PimdirEnumerated, Self::Error> {
        let item = self.item();
        let meta = PimdirRemoteMeta {
            link_id: item.link_id,
            summary: None,
            sort_key: PimdirSortKey::default(),
            body: None,
            server_object_id: None,
        };
        let meta = match item.body {
            Some(body) => meta.with_body(body),
            None => meta,
        };
        let items = vec![PimdirRemoteItem {
            handle: item.handle,
            flags: PimdirFlags::default(),
            revision: item.revision,
            meta,
        }];
        Ok(PimdirEnumerated::Page(PimdirRemoteSnapshot::round(
            items, None,
        )))
    }

    fn fetch(
        &mut self,
        _collection: &PimdirCollectionId,
        _handles: Vec<PimdirHandle>,
        _tier: PimdirTier,
    ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
        Ok(vec![self.item()])
    }

    fn push(
        &mut self,
        _collection: &PimdirCollectionId,
        changes: Vec<PimdirChange>,
    ) -> Result<Vec<PimdirPushResult>, Self::Error> {
        let mut results = Vec::new();
        for change in changes {
            let handle = change.handle().clone();
            match change.kind {
                PimdirChangeKind::Update { object, .. } => {
                    let body = self.blobs.blobs().get(&object).unwrap().unwrap();
                    self.sent.push(String::from_utf8(body.clone()).unwrap());
                    self.body = body;
                }
                PimdirChangeKind::Remove { .. } => self.removed += 1,
                kind => panic!("the other endpoint was asked {kind:?}"),
            }
            results.push(PimdirPushResult {
                handle,
                outcome: PimdirPushOutcome::Accepted,
                assigned: None,
                revision: Some(String::from("w2")),
                relocated: false,
            });
        }
        Ok(results)
    }
}

/// The other endpoint's sync, pulling or pushing.
fn sync_other(store: &mut PimdirSourceStore, other: &mut Other, push: bool) {
    let verb = sync_verb(
        CAL,
        push,
        super::PimdirPushRights::all(),
        super::PimdirConflictPolicy::Manual,
        PimdirScope::unbounded(),
        false,
    );
    run_verb(store, other, verb).expect("the other sync");
}

/// An exception before the floor of an open series stays on the other
/// endpoint, the Graph read leaving it out.
///
/// One inside the window Graph no longer lists is deleted there, and the
/// series is still noted clipped.
#[test]
fn an_exception_before_the_floor_of_a_clipped_series_is_not_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let mut graph = open_store(dir.path(), "graph", "a").unwrap();
    graph.ensure_collection(CAL, "text/calendar").unwrap();
    let mut other_store = open_store(dir.path(), "other", "a").unwrap();
    let blobs = graph.blob_stage();

    let today = Timestamp::now().to_zoned_datetime(TimeZone::UTC).date();
    let next = today.checked_add(1.year()).unwrap().year();

    let clipped = pull_graph(
        &mut graph,
        run("k1", "Birthday", vec![exception(2012), exception(next)]),
    );
    assert_eq!(clipped.len(), 1, "{clipped:?}");

    let bound = load_side(&graph, CAL).unwrap().remove(0);
    let agreed = blobs.blobs().get(&bound.object.unwrap()).unwrap().unwrap();
    let agreed_text = String::from_utf8(agreed.clone()).unwrap();
    assert!(agreed_text.contains("Party 2012"), "{agreed_text}");
    assert!(
        agreed_text.contains(&format!("Party {next}")),
        "{agreed_text}"
    );

    let mut other = Other {
        link_id: bound.link_id.unwrap(),
        body: agreed,
        blobs: blobs.clone(),
        sent: Vec::new(),
        removed: 0,
    };
    sync_other(&mut other_store, &mut other, false);

    let clipped = pull_graph(&mut graph, run("k2", "Birthday!", Vec::new()));
    assert_eq!(clipped.len(), 1, "the series is still reported clipped");

    sync_other(&mut other_store, &mut other, true);

    assert_eq!(other.removed, 0, "nothing deleted");
    let [sent] = other.sent.as_slice() else {
        panic!("one update expected: {:?}", other.sent);
    };
    assert!(sent.contains("SUMMARY:Birthday!"), "{sent}");
    assert!(
        sent.contains("Party 2012"),
        "the 2012 exception went: {sent}"
    );
    assert!(!sent.contains(&format!("Party {next}")), "{sent}");
}
