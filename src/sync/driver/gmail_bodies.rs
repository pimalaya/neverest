//! # Gmail hydration
//!
//! Hydration of a Gmail source, whose message ids name a message across
//! its labels, over scripted sessions: a message under two labels is
//! downloaded once, a body gone is owed nothing more for the run, and the
//! sizes the store records plan the reads.

use std::{
    path::Path,
    sync::{Arc, Mutex, atomic::AtomicU64},
};

use io_gmail::v1::rest::messages::encode_raw;
use io_pimdir::{
    change::PimdirWriteOp,
    client::PimdirSourceStore,
    collection::{PimdirCollectionId, PimdirScope},
    object::PimdirHash,
    placement::{
        PimdirBase, PimdirFlags, PimdirHandle, PimdirLevel, PimdirLinkId, PimdirPlacement,
        PimdirSortKey, PimdirStatus,
    },
    summary::{PimdirSummary, mail::PimdirMailSummary},
};
use pimalaya_stream::tls::Tls;
use secrecy::SecretString;

use super::{
    Authority, GoneBodies, Noted, Pacing, SourceCtx, hydrate_plans, open_store, phase2_hydrate,
    projection_view, sync_side,
};
use crate::{
    account::{GmailAccount, SourceAccount, SourceAccountBackend},
    client::{Client, Pool},
    config::AccountConfig,
    gmail::{
        client::scripted::{batch_answer, scripted},
        run::GmailRun,
    },
    scripted::{Server, ok},
    sync::event::SyncEvent,
    sync::progress::Progress,
    throttle::Throttle,
};

const INBOX: &str = "gmail/INBOX";
const WORK: &str = "gmail/Work";

/// A message listed under `collection`, its body owed, of `size`
/// octets by its summary.
fn owed(collection: &str, handle: &str, size: u64) -> PimdirWriteOp {
    PimdirWriteOp::UpsertPlacement(PimdirPlacement {
        collection: PimdirCollectionId(collection.into()),
        handle: PimdirHandle(handle.into()),
        link_id: Some(PimdirLinkId(format!("{handle}@example.org"))),
        object: None,
        level: PimdirLevel::Meta,
        summary: Some(PimdirSummary::Mail(PimdirMailSummary {
            size: Some(size),
            ..Default::default()
        })),
        sort_key: PimdirSortKey::default(),
        flags: PimdirFlags::default(),
        status: PimdirStatus::Clean,
        conflict_revision: None,
        conflict_object: None,
        base: Some(PimdirBase {
            flags: PimdirFlags::default(),
            revision: None,
            object: None,
        }),
        origin: None,
    })
}

fn raw(id: &str) -> serde_json::Value {
    let source = format!("Message-ID: <{id}@example.org>\r\n\r\nBody of {id}\r\n");
    serde_json::json!({ "id": id, "raw": encode_raw(source.as_bytes()) })
}

fn gone() -> serde_json::Value {
    serde_json::json!({ "error": { "code": 404, "message": "Requested entity was not found." } })
}

/// The store of a Gmail account syncing `INBOX` and `Work`, holding
/// `placements`.
fn store_holding(dir: &Path, placements: Vec<PimdirWriteOp>) -> PimdirSourceStore {
    let mut store = open_store(dir, "gmail", "gmail").unwrap();
    for collection in [INBOX, WORK] {
        store
            .ensure_collection(collection, "message/rfc822")
            .unwrap();
    }
    store.write(placements).unwrap();
    store
}

/// One connection to a scripted Gmail, its API and batch sessions.
pub(super) fn connection() -> (SourceCtx, Server, Server) {
    let (client, api, batch) = scripted();
    let config: AccountConfig = toml::from_str("gmail.auth.token.raw = \"x\"").unwrap();
    let perms = config.sources().unwrap()["gmail"].permissions();
    let account = SourceAccount {
        backend: SourceAccountBackend::Gmail(GmailAccount {
            token: SecretString::from(String::from("token")),
            user_id: String::from("me"),
            tls: Tls::default(),
            proxy: pimalaya_stream::proxy::Proxy::None,
            run: Arc::new(GmailRun::new(Default::default())),
        }),
        #[cfg(feature = "smtp")]
        smtp: None,
        throttle: Arc::new(Throttle::new("gmail")),
        downloaded: Arc::new(AtomicU64::new(0)),
    };
    let ctx = SourceCtx {
        name: String::from("gmail"),
        namespace: String::from("gmail"),
        pool: Pool::of(account, Client::Gmail(Box::new(client))),
        perms,
        authority: Authority::Shared,
        refused: Vec::new(),
        rejected: Vec::new(),
        scope: PimdirScope::unbounded(),
        dry_run: false,
        accept_absences: false,
        capped: false,
        noted: Noted::default(),
    };
    (ctx, api, batch)
}

/// Phase 2 over `collections`, the run's gone bodies `gone`.
fn hydrate(
    store: &mut PimdirSourceStore,
    ctx: &mut SourceCtx,
    collections: &[String],
    gone: &GoneBodies,
) {
    hydrate_reporting(store, ctx, collections, gone, Progress::default());
}

/// [`hydrate`], reporting to `progress`.
fn hydrate_reporting(
    store: &mut PimdirSourceStore,
    ctx: &mut SourceCtx,
    collections: &[String],
    gone: &GoneBodies,
    progress: Progress<'_>,
) {
    let plans = hydrate_plans(store, "gmail", collections, None, gone).unwrap();
    let blobs = store.blob_stage();
    phase2_hydrate(
        "gmail",
        plans,
        &Pacing::default(),
        std::slice::from_mut(ctx),
        std::slice::from_mut(store),
        &blobs,
        Some(collections),
        gone,
        progress,
    )
    .unwrap();
}

/// Each placement of `collection` and the body it holds.
fn bodies(store: &PimdirSourceStore, collection: &str) -> Vec<(String, Option<PimdirHash>)> {
    let mut bodies: Vec<_> = projection_view(store, collection, "gmail")
        .unwrap()
        .into_iter()
        .map(|placement| (placement.handle.0, placement.object))
        .collect();
    bodies.sort();
    bodies
}

/// A message under two labels downloads once and lands under both.
///
/// One gone since it was listed leaves the run complete, is asked once and
/// never again. The sizes come from the store: no minimal read asks for them.
#[test]
fn a_message_under_two_labels_downloads_once_and_one_gone_is_owed_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_holding(
        dir.path(),
        vec![
            owed(INBOX, "m1", 40),
            owed(INBOX, "m2", 40),
            owed(WORK, "m1", 40),
            owed(WORK, "m3", 40),
        ],
    );
    let (mut ctx, api, batch) = connection();
    batch.answer(batch_answer(&[
        (0, "200 OK", raw("m1")),
        (1, "200 OK", raw("m2")),
    ]));
    batch.answer(batch_answer(&[(0, "404 Not Found", gone())]));

    let collections = vec![INBOX.to_string(), WORK.to_string()];
    let run = GoneBodies::default();
    hydrate(&mut store, &mut ctx, &collections, &run);

    let requests = batch.lines();
    assert_eq!(requests.len(), 2, "m1 and m2, then m3: {requests:?}");
    assert!(api.lines().is_empty(), "nothing read alone");
    let inbox = bodies(&store, INBOX);
    let work = bodies(&store, WORK);
    assert!(inbox.iter().all(|(_, body)| body.is_some()), "{inbox:?}");
    assert_eq!(work[0].0, "m1");
    assert_eq!(work[0].1, inbox[0].1, "one body for both labels");
    assert_eq!(work[1], (String::from("m3"), None), "m3 stays as listed");

    hydrate(&mut store, &mut ctx, &collections, &run);
    assert_eq!(batch.lines().len(), 2, "nothing asked again");
    assert_eq!(batch.left(), 0);
}

/// Under `--json`, the download prints its start, then its progress.
///
/// A caller sees the total before the first body. Text mode prints the same
/// counts, as plain lines.
#[test]
fn a_download_prints_its_progress_as_events() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_holding(
        dir.path(),
        vec![owed(INBOX, "m1", 40), owed(INBOX, "m2", 40)],
    );
    let (mut ctx, _, batch) = connection();
    batch.answer(batch_answer(&[
        (0, "200 OK", raw("m1")),
        (1, "200 OK", raw("m2")),
    ]));

    let seen = Mutex::new(Vec::new());
    let sink = |event| seen.lock().unwrap().push(event);
    let collections = vec![INBOX.to_string()];
    let progress = Progress::events(&sink);
    hydrate_reporting(
        &mut store,
        &mut ctx,
        &collections,
        &GoneBodies::default(),
        progress,
    );

    let progressed = |done| SyncEvent::DownloadProgressed {
        source: "gmail".into(),
        done,
        total: 2,
    };
    let seen = seen.into_inner().unwrap();
    assert_eq!(seen, [progressed(0), progressed(1), progressed(2)]);

    let text: String = seen
        .iter()
        .filter(|event| event.is_coarse())
        .map(SyncEvent::to_string)
        .collect();
    assert_eq!(
        text,
        "Downloading gmail (0/2)\nDownloading gmail (1/2)\nDownloading gmail (2/2)\n"
    );
}

/// A message a label holds a body for, listed under another label
/// later, takes that body with no download.
#[test]
fn a_body_held_under_one_label_serves_another_without_a_download() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_holding(dir.path(), vec![owed(INBOX, "m1", 40)]);
    let (mut ctx, _, batch) = connection();
    batch.answer(batch_answer(&[(0, "200 OK", raw("m1"))]));
    hydrate(
        &mut store,
        &mut ctx,
        &[INBOX.to_string()],
        &GoneBodies::default(),
    );
    assert_eq!(batch.lines().len(), 1);

    store.write(vec![owed(WORK, "m1", 40)]).unwrap();
    let collections = vec![INBOX.to_string(), WORK.to_string()];
    hydrate(&mut store, &mut ctx, &collections, &GoneBodies::default());

    assert_eq!(batch.lines().len(), 1, "no second download");
    let inbox = bodies(&store, INBOX);
    let work = bodies(&store, WORK);
    assert!(work[0].1.is_some(), "{work:?}");
    assert_eq!(work[0].1, inbox[0].1);
}

/// A message the store binds in the inbox, dated `date` by its `Date`,
/// none for an undated draft.
fn dated(handle: &str, date: Option<&str>) -> PimdirWriteOp {
    let PimdirWriteOp::UpsertPlacement(mut placement) = owed(INBOX, handle, 40) else {
        unreachable!("owed upserts a placement");
    };
    if let Some(PimdirSummary::Mail(mail)) = &mut placement.summary {
        mail.date = date.map(String::from);
    }
    placement.sort_key = PimdirSortKey(date.unwrap_or_default().to_owned());
    PimdirWriteOp::UpsertPlacement(placement)
}

fn ids(ids: &[&str]) -> Vec<u8> {
    let messages: Vec<_> = ids
        .iter()
        .map(|id| serde_json::json!({ "id": id }))
        .collect();
    ok(serde_json::json!({ "messages": messages }))
}

/// A scoped round keeps what `after:` misses (pimdir SYNC §4, §5).
///
/// `after:` reads the arrival, so an undated draft received before the window
/// and a message dated ahead of its arrival are unlisted; the round states it
/// is narrowed and its last page hands the label's ids, naming both.
#[test]
fn a_scoped_round_keeps_what_its_arrival_filter_misses() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_holding(
        dir.path(),
        vec![
            dated("recent", Some("2026-10-20T10:00:00Z")),
            dated("draft", None),
            dated("ahead", Some("2030-01-01T10:00:00Z")),
        ],
    );
    let blobs = store.blob_stage();
    let (mut ctx, api, batch) = connection();
    ctx.scope = PimdirScope::since("2026-09-01T00:00:00Z");

    api.answer(ok(serde_json::json!({
        "emailAddress": "user@example.org",
        "historyId": "500"
    })));
    api.answer(ids(&["recent"]));
    for _ in 0..3 {
        api.answer(ids(&[]));
    }
    api.answer(ids(&["ahead", "draft", "recent"]));

    let (report, _) = sync_side(INBOX, &mut ctx, &mut store, &blobs, false, None).unwrap();

    assert_eq!(report.parked, None);
    let held: Vec<String> = bodies(&store, INBOX)
        .into_iter()
        .map(|(handle, _)| handle)
        .collect();
    assert_eq!(held, ["ahead", "draft", "recent"]);
    let requests = api.lines();
    let enumeration = requests.last().unwrap();
    assert!(enumeration.contains("/messages"), "{requests:?}");
    assert!(!enumeration.contains("q="), "no date filter: {requests:?}");
    assert_eq!(api.left() + batch.left(), 0, "{requests:?}");
}

/// Gmail for an inbox holding `messages`, ids and arrivals in epoch seconds.
///
/// `after:` and `before:` read the arrival (`internalDate`), never the
/// `Date`; the flag labels hold none, and the history is empty.
fn inbox(messages: Vec<(&'static str, i64)>) -> impl FnMut(&str) -> Vec<u8> + Send {
    move |request| {
        let target = request.split(' ').nth(1).unwrap_or_default();
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        if path.ends_with("/profile") {
            return ok(serde_json::json!({
                "emailAddress": "user@example.org",
                "historyId": "500"
            }));
        }
        if path.ends_with("/history") {
            return ok(serde_json::json!({ "history": [], "historyId": "500" }));
        }

        let pairs: Vec<(&str, &str)> = query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .collect();
        let labels = pairs.iter().filter(|(key, _)| *key == "labelIds").count();
        let search = pairs
            .iter()
            .find(|(key, _)| *key == "q")
            .map(|(_, value)| value.replace("%3A", ":").replace(['+'], " "))
            .unwrap_or_default();
        let bound = |term: &str| {
            let mut terms = search.split(' ');
            terms.find_map(|word| word.strip_prefix(term)?.parse::<i64>().ok())
        };
        let (after, before) = (bound("after:"), bound("before:"));
        let listed: Vec<&str> = messages
            .iter()
            .filter(|_| labels == 1)
            .filter(|(_, at)| after.is_none_or(|after| *at > after))
            .filter(|(_, at)| before.is_none_or(|before| *at < before))
            .map(|(id, _)| *id)
            .collect();
        ids(&listed)
    }
}

/// A message received ten months after its `Date` stays bound when a widening
/// lists its band.
///
/// Gmail's `before:` reads the arrival, so a band round narrowed by it missed
/// the message, found it absent (pimdir SYNC §5) and dropped it.
#[test]
fn a_band_round_keeps_a_message_received_after_its_date() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store_holding(
        dir.path(),
        vec![
            dated("early", Some("2026-09-01T10:00:00Z")),
            dated("late", Some("2026-01-01T00:00:00Z")),
        ],
    );
    let blobs = store.blob_stage();
    let inbox = || inbox(vec![("early", 1_788_256_800), ("late", 1_794_268_800)]);

    let mut requests = Vec::new();
    for scope in [
        PimdirScope::since("2026-10-16T00:00:00Z"),
        PimdirScope::unbounded(),
    ] {
        let (mut ctx, api, batch) = connection();
        ctx.scope = scope;
        api.respond(inbox());
        sync_side(INBOX, &mut ctx, &mut store, &blobs, false, None).unwrap();
        assert_eq!(batch.lines(), Vec::<String>::new());
        requests = api.lines();
    }

    let coverage = store.list_coverage(INBOX).unwrap();
    let closed = coverage[0].coverage.as_ref().expect("the band closed");
    assert_eq!(closed.scope, PimdirScope::unbounded(), "{requests:?}");
    let held: Vec<String> = bodies(&store, INBOX)
        .into_iter()
        .map(|(handle, _)| handle)
        .collect();
    assert_eq!(held, ["early", "late"], "{requests:?}");
}
