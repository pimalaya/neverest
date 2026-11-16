//! # Gmail round heads
//!
//! Heads of a Gmail round (pimdir SYNC §4, §5), over scripted sessions: a
//! run resuming an open round lists the label's history since the round
//! began before its next page, so what arrives during a long round shows
//! at once.

use std::{collections::BTreeSet, path::Path};

use io_pimdir::{
    client::PimdirSourceStore,
    collection::{PimdirCheckpoint, PimdirCollectionId},
    load::{PimdirLoadSelection, PimdirLoaded},
    placement::PimdirHandle,
    remote::PimdirListing,
};

use super::{gmail_bodies::connection, open_store, projection_view, sync_side};
use crate::{
    gmail::client::scripted::batch_answer,
    scripted::{Server, json, ok},
    sync::{Pager, Rest},
};

const INBOX: &str = "gmail/INBOX";

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

/// Pauses before a round's second page when `pause`, recording every
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
    projection_view(store, INBOX, "gmail")
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

fn metadata(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "labelIds": ["INBOX"],
        "sizeEstimate": 1000,
        "payload": {"headers": [
            {"name": "Message-ID", "value": format!("<{id}@example.org>")},
            {"name": "Date", "value": "Mon, 6 Jul 2026 14:00:00 +0200"},
        ]}
    })
}

fn listed(ids: &[&str], token: Option<&str>) -> Vec<u8> {
    let messages: Vec<_> = ids
        .iter()
        .map(|id| serde_json::json!({ "id": id }))
        .collect();
    let mut page = serde_json::json!({ "messages": messages });
    if let Some(token) = token {
        page["nextPageToken"] = token.into();
    }
    ok(page)
}

/// One sync of the inbox over a fresh run, scripted by `script`.
fn sync(
    store: &mut PimdirSourceStore,
    pause: bool,
    script: impl FnOnce(&Server, &Server),
) -> (bool, Vec<Rested>, Vec<String>) {
    let (mut ctx, api, batch) = connection();
    script(&api, &batch);
    let mut watch = Watch {
        pause,
        rests: Vec::new(),
    };
    let blobs = store.blob_stage();
    let (_, paused) =
        sync_side(INBOX, &mut ctx, store, &blobs, false, Some(&mut watch)).expect("the sync");
    assert_eq!(
        api.left() + batch.left(),
        0,
        "every answer asked for: {:?} {:?}",
        api.lines(),
        batch.lines()
    );
    (paused, watch.rests, api.lines())
}

/// A store syncing the inbox whose round's first page landed: m2 and m1
/// listed, the round open at its page token from history id 500.
fn first_page_landed(dir: &Path) -> PimdirSourceStore {
    let mut store = open_store(dir, "gmail", "gmail").unwrap();
    store.ensure_collection(INBOX, "message/rfc822").unwrap();
    let (paused, _, _) = sync(&mut store, true, |api, batch| {
        api.answer(ok(serde_json::json!({
            "emailAddress": "user@example.org",
            "historyId": "500"
        })));
        api.answer(listed(&["m2", "m1"], Some("t2")));
        batch.answer(batch_answer(&[
            (0, "200 OK", metadata("m2")),
            (1, "200 OK", metadata("m1")),
        ]));
    });
    assert!(paused, "stopped before the second page");
    assert_eq!(handles(&store), names(&["m1", "m2"]));
    let round = loaded(&store).round.expect("the round is left open");
    assert_eq!(round.checkpoint, Some(PimdirCheckpoint(b"500".to_vec())));
    store
}

/// An arrival while the round is open lands next run before the round's next
/// page (pimdir vector 62).
///
/// It comes from the history since the round began and survives the close,
/// which takes the head's history id as the source's next checkpoint.
#[test]
fn a_head_lands_an_arrival_the_round_keeps() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = first_page_landed(dir.path());

    let (paused, rests, requests) = sync(&mut store, false, |api, batch| {
        api.answer(ok(serde_json::json!({
            "history": [{ "id": "510", "messagesAdded": [{
                "message": { "id": "m9", "labelIds": ["INBOX", "UNREAD"] }
            }]}],
            "historyId": "520"
        })));
        batch.answer(batch_answer(&[(0, "200 OK", metadata("m9"))]));
        api.answer(listed(&["m3"], None));
        batch.answer(batch_answer(&[(0, "200 OK", metadata("m3"))]));
    });
    assert!(!paused);

    assert!(requests[0].contains("/history"), "{requests:?}");
    assert!(requests[0].contains("startHistoryId=500"), "{requests:?}");
    assert!(requests[1].contains("pageToken=t2"), "{requests:?}");
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
    assert!(loaded.coverage.is_some());
    assert_eq!(
        loaded.checkpoint,
        Some(PimdirCheckpoint(b"520".to_vec())),
        "the head's history id"
    );

    let (_, _, requests) = sync(&mut store, false, |api, _| {
        api.answer(ok(serde_json::json!({ "history": [], "historyId": "530" })));
    });
    assert!(requests[0].contains("startHistoryId=520"), "{requests:?}");
    assert_eq!(handles(&store), names(&["m1", "m2", "m3", "m9"]));
}

/// A message the head states deleted leaves the store at once, before
/// the round's next page.
#[test]
fn a_deletion_in_the_head_drops_the_member_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = first_page_landed(dir.path());

    let (_, rests, _) = sync(&mut store, false, |api, batch| {
        api.answer(ok(serde_json::json!({
            "history": [{ "id": "510", "messagesDeleted": [{
                "message": { "id": "m1", "labelIds": ["INBOX"] }
            }]}],
            "historyId": "520"
        })));
        api.answer(listed(&["m3"], None));
        batch.answer(batch_answer(&[(0, "200 OK", metadata("m3"))]));
    });

    assert!(rests[1].landed.is_empty(), "the head named no member");
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
    assert_eq!(rests[1].held, names(&["m2"]), "m1 dropped by the head");
    assert_eq!(handles(&store), names(&["m2", "m3"]));
    let loaded = loaded(&store);
    assert!(loaded.round.is_none());
    assert_eq!(loaded.checkpoint, Some(PimdirCheckpoint(b"520".to_vec())));
}

/// A head whose history id Gmail no longer holds restarts the round
/// from its first page, which takes a fresh history id.
#[test]
fn an_expired_head_restarts_the_round() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = first_page_landed(dir.path());

    let (paused, _, requests) = sync(&mut store, false, |api, batch| {
        api.answer(json(
            "404 Not Found",
            r#"{"error":{"code":404,"message":"Requested entity was not found.","status":"NOT_FOUND"}}"#,
        ));
        api.answer(ok(serde_json::json!({
            "emailAddress": "user@example.org",
            "historyId": "600"
        })));
        api.answer(listed(&["m9", "m3", "m2", "m1"], None));
        api.answer(listed(&[], None));
        api.answer(listed(&[], None));
        api.answer(listed(&[], None));
        batch.answer(batch_answer(&[
            (0, "200 OK", metadata("m9")),
            (1, "200 OK", metadata("m3")),
        ]));
    });
    assert!(!paused);
    assert!(requests[0].contains("startHistoryId=500"), "{requests:?}");
    assert!(requests[1].contains("/profile"), "{requests:?}");
    assert!(!requests[2].contains("pageToken"), "{requests:?}");

    assert_eq!(handles(&store), names(&["m1", "m2", "m3", "m9"]));
    let loaded = loaded(&store);
    assert!(loaded.round.is_none(), "the restarted round closed");
    assert!(loaded.coverage.is_some());
    assert_eq!(
        loaded.checkpoint,
        Some(PimdirCheckpoint(b"600".to_vec())),
        "the restarted round's own"
    );
}
