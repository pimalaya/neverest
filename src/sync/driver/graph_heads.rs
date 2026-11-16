//! # Graph round heads
//!
//! Band heads of a Graph mail round (pimdir SYNC §4, §5), through the
//! driver over a scripted Graph: a run resuming an open round lists what
//! Graph received since two days before the round began, before its next
//! page, and the round's last page lists that again after the delta link,
//! so an arrival shows at once and survives the round's close.

use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, atomic::AtomicU64},
};

use chrono::{DateTime, SecondsFormat, TimeDelta};
use io_pimdir::{
    client::PimdirSourceStore,
    collection::{PimdirCollectionId, PimdirScope},
    load::{PimdirLoadSelection, PimdirLoaded},
    placement::PimdirHandle,
    remote::PimdirListing,
};
use pimalaya_stream::tls::Tls;
use secrecy::SecretString;

use super::{Authority, Noted, SourceCtx, open_store, projection_view, sync_side};
use crate::{
    account::{MsgraphAccount, SourceAccount, SourceAccountBackend},
    client::{Client, Pool},
    config::AccountConfig,
    msgraph::client::{GraphClient, GraphKind, SizeHints},
    scripted::{Server, Step},
    sync::{Pager, Rest},
    throttle::Throttle,
};

const INBOX: &str = "msgraph/Inbox";
const DELTA: &str = "https://graph.microsoft.com/v1.0/me/mailFolders/id-inbox/messages/delta";

/// What the listing looked like each time it rested.
struct Rested {
    /// The members of the page enumerated last.
    landed: Vec<String>,
    resumed: bool,
    /// The listing about to be asked for.
    next: Option<PimdirListing>,
    /// What the store held then.
    held: BTreeSet<String>,
}

/// Pauses before a round's next page when `pause`, recording every
/// rest.
struct Watch {
    pause: bool,
    rests: Vec<Rested>,
}

impl Pager for Watch {
    fn at_rest(
        &mut self,
        store: &mut PimdirSourceStore,
        _collection: &PimdirCollectionId,
        landed: Vec<PimdirHandle>,
        resumed: bool,
        next: Option<&PimdirListing>,
    ) -> Rest {
        self.rests.push(Rested {
            landed: landed.into_iter().map(|handle| handle.0).collect(),
            resumed,
            next: next.cloned(),
            held: handles(store),
        });
        match (self.pause, next) {
            (
                true,
                Some(PimdirListing::Round {
                    cursor: Some(_), ..
                }),
            ) => Rest::Pause,
            _ => Rest::Go,
        }
    }

    fn listing(&mut self) {}

    fn listed(&mut self) {}
}

fn handles(store: &PimdirSourceStore) -> BTreeSet<String> {
    projection_view(store, INBOX, "msgraph")
        .unwrap()
        .into_iter()
        .map(|placement| placement.handle.0)
        .collect()
}

fn names(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

fn loaded(store: &PimdirSourceStore) -> PimdirLoaded {
    store
        .load(
            &PimdirCollectionId(INBOX.to_string()),
            &PimdirLoadSelection::All,
        )
        .unwrap()
}

/// A message as Graph's summary `$select` reads it.
fn message(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "subject": format!("Subject {id}"),
        "sentDateTime": "2026-10-06T08:00:00Z",
        "receivedDateTime": "2026-10-06T08:00:01Z",
        "isRead": false,
        "isDraft": false,
        "flag": { "flagStatus": "notFlagged" },
        "hasAttachments": false,
        "internetMessageId": format!("<{id}@example.org>"),
    })
}

/// A page of `ids`, with its next link or, last, the delta link.
fn page(ids: &[&str], next: Option<&str>) -> Step {
    let value: Vec<_> = ids.iter().map(|id| message(id)).collect();
    let mut page = serde_json::json!({ "value": value });
    match next {
        Some(token) => page["@odata.nextLink"] = format!("{DELTA}?$skiptoken={token}").into(),
        None => page["@odata.deltaLink"] = format!("{DELTA}?$deltatoken=d1").into(),
    }
    Step::ok(page)
}

/// A page of the listing by arrival: `ids`, and no next link.
fn arrivals(ids: &[&str]) -> Step {
    let value: Vec<_> = ids.iter().map(|id| message(id)).collect();
    Step::ok(serde_json::json!({ "value": value }))
}

/// One sync of the inbox over a fresh run whose one connection answers
/// `script`, pausing before the round's next page when `pause`: whether
/// it paused, its rests and the request lines it sent.
fn sync(
    store: &mut PimdirSourceStore,
    pause: bool,
    script: Vec<Step>,
) -> (bool, Vec<Rested>, Vec<String>) {
    let (client, server) = GraphClient::scripted(vec![script], &[("Inbox", "id-inbox")]);
    let mut ctx = context(client);
    let mut watch = Watch {
        pause,
        rests: Vec::new(),
    };
    let blobs = store.blob_stage();
    let (_, paused) =
        sync_side(INBOX, &mut ctx, store, &blobs, false, Some(&mut watch)).expect("the sync");
    (paused, watch.rests, requests(&server))
}

fn requests(server: &Server) -> Vec<String> {
    server.lines()
}

fn context(client: GraphClient) -> SourceCtx {
    let config: AccountConfig =
        toml::from_str("msgraph.auth.token.raw = \"x\"\nmsgraph.mail = true").unwrap();
    let perms = config.sources().unwrap()["msgraph"].permissions();
    let account = SourceAccount {
        backend: SourceAccountBackend::Msgraph(MsgraphAccount {
            kind: GraphKind::Mail,
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
        name: String::from("msgraph"),
        namespace: String::from("msgraph"),
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

/// A store syncing the inbox whose whole-folder round's first page
/// landed: m1 and m2 listed, the round open at its next link.
fn first_page_landed(dir: &Path) -> PimdirSourceStore {
    let mut store = open_store(dir, "msgraph", "msgraph").unwrap();
    store.ensure_collection(INBOX, "message/rfc822").unwrap();
    let (paused, _, requests) = sync(&mut store, true, vec![page(&["m1", "m2"], Some("t2"))]);
    assert!(paused, "stopped before the second page");
    assert_eq!(requests.len(), 1, "a run opening a round lists no head");
    assert_eq!(handles(&store), names(&["m1", "m2"]));
    assert!(loaded(&store).round.is_some(), "the round is left open");
    store
}

/// The `$filter` a band head of the open round asks for: received from
/// two days before the round opened, as the request line encodes it.
fn head_filter(store: &PimdirSourceStore) -> String {
    let coverage = store.list_coverage(INBOX).unwrap();
    let since = &coverage[0]
        .round
        .as_ref()
        .expect("a round is open")
        .started_at;
    let from = DateTime::parse_from_rfc3339(since).unwrap().to_utc() - TimeDelta::days(2);
    let from = from.to_rfc3339_opts(SecondsFormat::Secs, true);
    format!("%24filter=receivedDateTime+ge+{}", from.replace(':', "%3A"))
}

/// A message arriving while the round is open lands at the next run,
/// before the round's next page, from the head; the round's last page
/// lists the head again after the delta link, so it survives the
/// round's close (pimdir vector 65).
#[test]
fn a_band_head_lands_an_arrival_the_round_keeps() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = first_page_landed(dir.path());
    let filter = head_filter(&store);

    let (paused, rests, requests) = sync(
        &mut store,
        false,
        vec![arrivals(&["m9"]), page(&["m3"], None), arrivals(&["m9"])],
    );
    assert!(!paused);

    assert_eq!(requests.len(), 3, "{requests:?}");
    for at in [0, 2] {
        assert!(
            requests[at].starts_with("GET /v1.0/me/mailFolders/id-inbox/messages?"),
            "{requests:?}"
        );
        assert!(requests[at].contains(&filter), "{requests:?}");
        assert!(
            requests[at].contains("%24orderby=receivedDateTime+desc"),
            "{requests:?}"
        );
    }
    assert!(requests[1].contains("skiptoken=t2"), "{requests:?}");

    assert!(rests[0].resumed, "the head opens a resumed round");
    assert!(matches!(rests[0].next, Some(PimdirListing::Head(_))));
    assert_eq!(rests[1].landed, ["m9"], "the head's arrival");
    assert!(
        matches!(
            rests[1].next,
            Some(PimdirListing::Round {
                cursor: Some(_),
                ..
            })
        ),
        "before the round's next page"
    );
    assert!(rests[1].held.contains("m9"), "readable already");

    assert_eq!(handles(&store), names(&["m1", "m2", "m3", "m9"]));
    let loaded = loaded(&store);
    assert!(loaded.round.is_none(), "the round closed");
    let checkpoint = loaded.checkpoint.expect("the delta link").0;
    assert_eq!(
        checkpoint,
        format!("summary {DELTA}?$deltatoken=d1").into_bytes()
    );
}

/// What a head named and Graph no longer holds when the round's last
/// page lists the head again is found absent when the round closes;
/// what it still holds is kept (pimdir vector 66).
#[test]
fn the_last_page_decides_what_a_band_head_named() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = first_page_landed(dir.path());

    // NOTE: the run stops after the head, the round's next page left
    // for the next run.
    let (paused, _, requests) = sync(&mut store, true, vec![arrivals(&["m9", "m8"])]);
    assert!(paused);
    assert_eq!(requests.len(), 1, "the head alone");
    assert_eq!(handles(&store), names(&["m1", "m2", "m8", "m9"]));
    assert!(loaded(&store).round.is_some());

    let (_, _, requests) = sync(
        &mut store,
        false,
        vec![arrivals(&["m9"]), page(&["m3"], None), arrivals(&["m9"])],
    );
    assert_eq!(requests.len(), 3, "{requests:?}");
    assert!(requests[1].contains("skiptoken=t2"), "{requests:?}");
    assert_eq!(
        handles(&store),
        names(&["m1", "m2", "m3", "m9"]),
        "m8 went before the last page"
    );
    assert!(loaded(&store).round.is_none(), "the round closed");
}
