//! [`io_replica::client::ReplicaRemote`] backed by one [`Client`], one
//! side of the sync.
//!
//! **Still mail-only.** The seam it sits on ([`crate::client`]) is
//! kind-neutral, but everything below is `message/rfc822`: the link id is a
//! `Message-ID`, the summary is the mail `v:1` schema, and the enumerate
//! narration is IMAP's. The kind seam (change `generic-pim-sync`, phase 2)
//! lifts the link-id and summary derivation out of here into one
//! implementation per media type, selected from [`Client::media_type`];
//! phase 3 then teaches `push` the mutable-content path this file rejects
//! outright today.
//!
//! `enumerate` reads the collection's UID+flag spine incrementally: with a stored
//! cursor (`(UIDVALIDITY, HIGHESTMODSEQ)`) and a QRESYNC server it does a QRESYNC
//! `SELECT`, which streams only the messages changed since the modseq and the
//! UIDs that vanished — nothing is fetched when nothing changed. Without a cursor
//! (or on a UIDVALIDITY change, or a non-QRESYNC server) it falls back to a full
//! `FETCH 1:* (UID FLAGS)` snapshot; io-replica derives what vanished by diffing
//! the complete snapshot against the stored placements. `fetch` resolves the link
//! id from the `Message-ID` header
//! (ENVELOPE only at the `Meta` tier, the raw body at `Full`); `push`
//! maps the four [`ReplicaChangeKind`] variants onto `store_flags`/
//! `delete_item`/`move_items`/`add_item_stream`.
//!
//! The link id is the message's `Message-ID` (prefixed `mid:`), falling
//! back to a `(subject, date, sender)` digest (prefixed `alt:`) when the
//! header is missing — so a headerless message still links across the
//! two sides. Bodies are content-addressed by the store's own hash
//! ([`PimdirBlobs::hasher`], the algorithm the store records); mail is
//! immutable, so `revision` is always `None` and content conflicts never
//! arise.
//!
//! (A CONDSTORE-only server without QRESYNC still full-enumerates today — the
//! `FETCH … CHANGEDSINCE` fallback is a later refinement.)

use std::{
    cmp::Reverse,
    collections::HashMap,
    io::{self, Write},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use anyhow::{Context, Result};
use crossbeam_queue::SegQueue;
use io_pimdir::{PimdirBlobWriter, PimdirBlobs, hash::PimdirHasher};
use io_replica::{
    change::{ReplicaChange, ReplicaChangeKind},
    client::ReplicaRemote,
    collection::{ReplicaCheckpoint, ReplicaCollectionId},
    object::ReplicaHash,
    placement::{ReplicaFlags, ReplicaHandle},
    remote::{
        ReplicaFetchedBody, ReplicaFetchedItem, ReplicaPushOutcome, ReplicaPushResult,
        ReplicaRemoteItem, ReplicaRemoteSnapshot, ReplicaTier,
    },
};
use log::warn;

use crate::{
    client::{Client, Pool},
    item::{
        flag::{Flag, FlagOp},
        summary::ItemSummary,
    },
    kind::Kind,
};

/// One side's remote seam over its connection [`Pool`].
///
/// Sequential verbs (enumerate, push, `Meta` fetch) run on the pool's primary
/// connection; a `Full` fetch borrows several of the pool's connections at once
/// to stream several bodies in parallel. The pool is persistent, so the extra
/// connections' auth is paid once for the whole run, not per batch.
pub struct PimRemote<'a> {
    /// The kind this side syncs, resolved once from
    /// [`Client::media_type`]. It selects the link-id and summary
    /// derivation; the driver has already refused an unknown or
    /// mismatched media type before any remote is built.
    kind: Kind,
    pool: &'a mut Pool,
    blob: PimdirBlobs,
    /// Called once per body streamed at the `Full` tier, so the driver can tick a
    /// per-message progress counter. `Sync` because the fetch pool calls it from
    /// several worker threads at once.
    on_body: Option<&'a (dyn Fn() + Sync)>,
    /// Handle → body octet size, from the store's already-fetched envelope meta
    /// (no server round trip). When present, `Full` fetches are ordered
    /// largest-first so the heavy messages go up front — the progress counter then
    /// crawls at the start and accelerates to a smooth finish (rather than
    /// stalling on a big message that happened to land last).
    sizes: HashMap<String, u64>,
}

impl<'a> PimRemote<'a> {
    pub fn new(pool: &'a mut Pool, blob: PimdirBlobs) -> Self {
        let kind = resolve_kind(pool);
        Self {
            kind,
            pool,
            blob,
            on_body: None,
            sizes: HashMap::new(),
        }
    }

    /// Like [`new`](Self::new) but ticks `on_body` after each `Full` body streams,
    /// and orders the `Full` fetch largest-first using `sizes` (handle → octets,
    /// from the store's envelope meta — no round trip). Pass an empty map to keep
    /// UID order.
    pub fn with_progress(
        pool: &'a mut Pool,
        blob: PimdirBlobs,
        on_body: &'a (dyn Fn() + Sync),
        sizes: HashMap<String, u64>,
    ) -> Self {
        let kind = resolve_kind(pool);
        Self {
            kind,
            pool,
            blob,
            on_body: Some(on_body),
            sizes,
        }
    }
}

/// The [`Kind`] a pool's backend syncs.
///
/// The driver refuses an unknown or cross-kind media type before opening any
/// remote (`check_kinds`), so this cannot legitimately fail; falling back to
/// [`Kind::Mail`] rather than panicking keeps a construction path that takes no
/// `Result` honest, and the warning names the media type if it ever does.
pub(crate) fn resolve_kind(pool: &mut Pool) -> Kind {
    let media_type = pool.primary().media_type();
    Kind::from_media_type(media_type).unwrap_or_else(|| {
        warn!("unknown media type `{media_type}`, deriving link ids as mail");
        Kind::Mail
    })
}

/// How many message bodies to request per batched `UID FETCH`. Larger cuts round
/// trips but coarsens the per-batch error/retry unit and the command size; 64 is
/// a balance that still turns a collection into ~N/64 fetches per connection.
pub(crate) const BATCH_SIZE: usize = 64;

/// Maps shared flags to protocol-neutral offline flag strings (their raw
/// wire spelling; a single side is internally consistent, and both sides
/// run the same normalization for the system flags that matter).
fn to_offline_flags<'f>(flags: impl IntoIterator<Item = &'f Flag>) -> ReplicaFlags {
    flags.into_iter().map(|f| f.raw()).collect()
}

/// The item flags of a known set. An unknown one (nothing has read the
/// item's markers) yields none, which is what a push of it would mean
/// anyway: neverest's backends all report markers as they enumerate, so
/// only a store written by another owner can carry one.
fn to_item_flags(flags: &ReplicaFlags) -> Vec<Flag> {
    let Some(flags) = flags.known() else {
        return Vec::new();
    };

    flags.iter().map(|s| Flag::from_raw(s.clone())).collect()
}

impl ReplicaRemote for PimRemote<'_> {
    type Error = anyhow::Error;

    fn enumerate(
        &mut self,
        collection: &ReplicaCollectionId,
        cursor: Option<ReplicaCheckpoint>,
    ) -> Result<ReplicaRemoteSnapshot, Self::Error> {
        let collection = collection.as_str();
        // NOTE: the cursor bytes are opaque to this seam; the backend decodes
        // its own encoding (IMAP `(UIDVALIDITY, HIGHESTMODSEQ)`, the Graph
        // delta link) and enumerates incrementally when it can, else a full
        // snapshot.
        let cursor = cursor.as_ref().map(|c| c.0.as_slice());
        let enumeration = self
            .pool
            .primary()
            .enumerate(collection, cursor)
            .with_context(|| format!("Enumerate `{collection}` error"))?;
        let items = enumeration
            .items
            .into_iter()
            .map(|entry| ReplicaRemoteItem {
                handle: ReplicaHandle::from(entry.id),
                flags: to_offline_flags(&entry.flags),
                revision: entry.revision,
            })
            .collect();
        let vanished = enumeration
            .vanished
            .into_iter()
            .map(ReplicaHandle::from)
            .collect();
        Ok(ReplicaRemoteSnapshot {
            items,
            vanished,
            complete: enumeration.complete,
            checkpoint: ReplicaCheckpoint(enumeration.checkpoint),
        })
    }

    fn fetch(
        &mut self,
        collection: &ReplicaCollectionId,
        handles: Vec<ReplicaHandle>,
        tier: ReplicaTier,
    ) -> Result<Vec<ReplicaFetchedItem>, Self::Error> {
        let collection = collection.as_str();

        match tier {
            ReplicaTier::Meta => self.fetch_meta(collection, handles),
            ReplicaTier::Full => self.fetch_full(collection, handles),
        }
    }

    fn push(
        &mut self,
        collection: &ReplicaCollectionId,
        changes: Vec<ReplicaChange>,
    ) -> Result<Vec<ReplicaPushResult>, Self::Error> {
        let collection = collection.as_str().to_string();
        let mut results = Vec::with_capacity(changes.len());

        for change in changes {
            let result = match change.kind {
                ReplicaChangeKind::SetFlags { handle, flags } => {
                    let email_flags = to_item_flags(&flags);
                    match self.pool.primary().store_flags(
                        &collection,
                        &[handle.as_str()],
                        &email_flags,
                        FlagOp::Set,
                    ) {
                        Ok(()) => accepted(handle, None),
                        Err(err) => rejected(handle, "store flags", err),
                    }
                }
                // NOTE: `link_id` is the identity a relocation would deliver,
                // which a consumer able to ask "does `to` already hold it?"
                // uses to demote the move to a plain delete. No backend seam
                // here answers that short of enumerating the destination, so
                // both halves of a move can deliver and the target may end up
                // holding the item twice, which the engine now freezes and
                // this crate reports rather than mispairing silently.
                ReplicaChangeKind::Remove {
                    handle,
                    to,
                    link_id: _,
                    if_match,
                } => match to {
                    Some(target) => {
                        let dest = target.as_str();
                        match self
                            .pool
                            .primary()
                            .move_items(&collection, dest, &[handle.as_str()])
                        {
                            Ok(()) => accepted(handle, None),
                            Err(err) => rejected(handle, "move item", err),
                        }
                    }
                    // `if_match` is the last-synced revision: a
                    // mutable-content backend deletes conditionally on it, so
                    // a delete cannot silently drop a body the remote changed
                    // after the last sync. Immutable-content backends ignore it.
                    None => match self.pool.primary().delete_item(
                        &collection,
                        handle.as_str(),
                        if_match.as_deref(),
                    ) {
                        Ok(()) => accepted(handle, None),
                        Err(err) => rejected(handle, "delete item", err),
                    },
                },
                ReplicaChangeKind::Add {
                    handle,
                    link_id,
                    flags,
                    object,
                    ..
                } => {
                    // NOTE: what a backend can address the new item by rides in
                    // the link id, and which prefix carries it is the kind's own
                    // business (a `mid:` Message-ID, a `uid:` card UID); a
                    // fallback link id (`alt:`, `hash:`) names nothing.
                    let hint = link_id.as_ref().and_then(|l| self.kind.link_hint(l));
                    self.append(&collection, handle, &flags, object, hint)
                }
                // An in-place body edit, on a mutable-content backend only.
                // The push is conditional on `if_match` (the last-synced
                // revision), so a remote that moved since is *rejected* rather
                // than overwritten: io-replica then re-merges and records the
                // divergence as a conflict for the consumer to settle. Mail
                // never gets here — its bodies are immutable, so io-replica
                // derives no `Update` — and both mail backends refuse the call
                // anyway.
                ReplicaChangeKind::Update {
                    handle,
                    object,
                    if_match,
                } => self.update(&collection, handle, object, if_match.as_deref()),
            };
            results.push(result);
        }

        Ok(results)
    }
}

/// The key into the pre-fetch cache: `(collection, handle)`.
pub type FetchKey = (String, String);

/// A [`ReplicaRemote`] for the Full-apply phase. Its `fetch` returns bodies the
/// global hydrate phase already streamed into the blob store, keyed by
/// `(collection, handle)`, so the per-collection `Full` upgrade does only index
/// writes — no network. A cache miss (a body the pre-fetch skipped or failed)
/// falls back to a real fetch on the wrapped [`PimRemote`], so a gap is
/// corrected rather than lost. `enumerate`/`push` are never reached by a `Full`
/// upgrade but delegate to the fallback for safety.
pub struct CachedFetchRemote<'a> {
    cache: &'a HashMap<FetchKey, ReplicaFetchedItem>,
    fallback: PimRemote<'a>,
}

impl<'a> CachedFetchRemote<'a> {
    pub fn new(cache: &'a HashMap<FetchKey, ReplicaFetchedItem>, fallback: PimRemote<'a>) -> Self {
        Self { cache, fallback }
    }
}

impl ReplicaRemote for CachedFetchRemote<'_> {
    type Error = anyhow::Error;

    fn enumerate(
        &mut self,
        collection: &ReplicaCollectionId,
        cursor: Option<ReplicaCheckpoint>,
    ) -> Result<ReplicaRemoteSnapshot, Self::Error> {
        self.fallback.enumerate(collection, cursor)
    }

    fn fetch(
        &mut self,
        collection: &ReplicaCollectionId,
        handles: Vec<ReplicaHandle>,
        tier: ReplicaTier,
    ) -> Result<Vec<ReplicaFetchedItem>, Self::Error> {
        let coll = collection.as_str();
        let mut items = Vec::with_capacity(handles.len());
        let mut misses = Vec::new();
        for handle in handles {
            match self.cache.get(&(coll.to_string(), handle.0.clone())) {
                Some(item) => items.push(item.clone()),
                None => misses.push(handle),
            }
        }
        if !misses.is_empty() {
            // A body the pre-fetch missed: fetch it for real (network) now.
            items.extend(self.fallback.fetch(collection, misses, tier)?);
        }
        Ok(items)
    }

    fn push(
        &mut self,
        collection: &ReplicaCollectionId,
        changes: Vec<ReplicaChange>,
    ) -> Result<Vec<ReplicaPushResult>, Self::Error> {
        self.fallback.push(collection, changes)
    }
}

impl PimRemote<'_> {
    /// Meta tier: a targeted ENVELOPE fetch of just the requested handles (no
    /// bodies, no whole-collection sweep), indexed by id, so they resolve their link
    /// id and summary cheaply and the cost scales with the change, not the collection.
    fn fetch_meta(
        &mut self,
        collection: &str,
        handles: Vec<ReplicaHandle>,
    ) -> Result<Vec<ReplicaFetchedItem>> {
        let ids: Vec<&str> = handles.iter().map(|h| h.as_str()).collect();
        let envelopes = self
            .pool
            .primary()
            .fetch_summaries(collection, &ids)
            .with_context(|| format!("Fetch envelopes `{collection}` error"))?;
        let by_id: HashMap<&str, &ItemSummary> =
            envelopes.iter().map(|e| (e.id.as_str(), e)).collect();

        let mut items = Vec::with_capacity(handles.len());
        for handle in handles {
            let Some(env) = by_id.get(handle.as_str()) else {
                continue;
            };
            // A kind with no cheap summary tier never reaches here: the driver
            // upgrades it straight to `Full` instead.
            let Some((link_id, meta, sort_key)) = self.kind.parse_summary(env) else {
                continue;
            };
            items.push(ReplicaFetchedItem {
                handle,
                link_id,
                meta,
                sort_key,
                body: None,
                revision: None,
            });
        }
        Ok(items)
    }

    /// Full tier: every body streamed straight into the blob store (never held
    /// whole), fetched in **batches** — one `UID FETCH <set> (UID BODY.PEEK[])`
    /// per batch instead of one command per message, so N bodies cost ~N/BATCH
    /// round trips. Batches are work-stolen across a bounded pool of connections;
    /// the engine serialises the index write afterwards.
    fn fetch_full(
        &mut self,
        collection: &str,
        mut handles: Vec<ReplicaHandle>,
    ) -> Result<Vec<ReplicaFetchedItem>> {
        if handles.is_empty() {
            return Ok(Vec::new());
        }
        if self.sizes.is_empty() {
            // No sizes: sort by numeric UID so consecutive ids collapse to ranges
            // in the FETCH command (a first sync's 1..N becomes a few ranges).
            handles.sort_by_key(|h| h.as_str().parse::<u64>().unwrap_or(u64::MAX));
        } else {
            // Largest-first: the heavy messages go up front, so the progress
            // counter starts slow and accelerates to a smooth finish instead of
            // freezing on a big message that landed last. Sizes come from the
            // store's envelope meta, so this costs no round trip.
            handles.sort_by_key(|h| Reverse(self.sizes.get(h.as_str()).copied().unwrap_or(0)));
        }

        let total = handles.len();
        let target = self.pool.max().min(total);
        let batches: Vec<Vec<ReplicaHandle>> = handles
            .chunks(BATCH_SIZE)
            .map(<[ReplicaHandle]>::to_vec)
            .collect();

        if target <= 1 {
            // One connection: run each batch serially on the primary.
            let blob = self.blob.clone();
            let mut items = Vec::with_capacity(total);
            for batch in &batches {
                items.extend(hydrate_batch(
                    self.kind,
                    self.pool.primary(),
                    collection,
                    batch,
                    &blob,
                    self.on_body,
                )?);
            }
            return Ok(items);
        }

        self.fetch_full_pooled(collection, batches, target)
    }

    /// Fetches `batches` across up to `target` of the pool's persistent
    /// connections: a shared queue of batches, each worker draining it on its own
    /// connection and issuing one batched `UID FETCH` per batch. Work-stealing
    /// balances load (a worker with heavy batches naturally takes fewer) with no
    /// size probe.
    fn fetch_full_pooled(
        &mut self,
        collection: &str,
        batches: Vec<Vec<ReplicaHandle>>,
        target: usize,
    ) -> Result<Vec<ReplicaFetchedItem>> {
        let queue: SegQueue<Vec<ReplicaHandle>> = SegQueue::new();
        for batch in batches {
            queue.push(batch);
        }
        let results: Mutex<Vec<ReplicaFetchedItem>> = Mutex::new(Vec::new());
        let failure: Mutex<Option<anyhow::Error>> = Mutex::new(None);
        let stop = AtomicBool::new(false);

        // Copy the kind before the worker borrow: `workers()` takes `self` mutably.
        let kind = self.kind;
        // Clone the cheap blob handle so the worker connections can borrow the
        // pool without also tying up `self.blob`.
        let blob = self.blob.clone();
        let clients = self.pool.workers(target)?;

        let queue_ref = &queue;
        let results_ref = &results;
        let failure_ref = &failure;
        let stop_ref = &stop;
        let blob_ref = &blob;
        let on_body = self.on_body;

        thread::scope(|scope| {
            for client in clients.iter_mut() {
                scope.spawn(move || {
                    while !stop_ref.load(Ordering::Relaxed) {
                        let Some(batch) = queue_ref.pop() else {
                            break;
                        };
                        match hydrate_batch(kind, client, collection, &batch, blob_ref, on_body) {
                            Ok(mut items) => results_ref.lock().unwrap().append(&mut items),
                            Err(err) => {
                                *failure_ref.lock().unwrap() = Some(err);
                                stop_ref.store(true, Ordering::Relaxed);
                                break;
                            }
                        }
                    }
                });
            }
        });

        if let Some(err) = failure.into_inner().unwrap() {
            return Err(err);
        }
        Ok(results.into_inner().unwrap())
    }
}

/// Streams one message's body straight into the blob store and reports the
/// object by reference (bounded memory); the link id and summary come from the
/// streamed header prefix. Shared by the serial and pooled fetch paths, and free
/// of `PimRemote` so a pool worker can call it on its own connection.
fn fetch_one_full(
    kind: Kind,
    client: &mut Client,
    collection: &str,
    handle: ReplicaHandle,
    blob: &PimdirBlobs,
) -> Result<ReplicaFetchedItem> {
    let writer = blob.writer().context("Open blob writer error")?;
    let mut sink = HydrateSink::new(writer, blob.hasher());
    let revision = client
        .get_item_stream(collection, handle.as_str(), &mut sink)
        .with_context(|| format!("Stream item `{}` in `{collection}` error", handle.as_str()))?;
    let (hash, size, header) = sink
        .finish()
        .with_context(|| format!("Commit body `{}` in `{collection}` error", handle.as_str()))?;

    let (link, meta, sort_key) = kind.parse_body(&header, size as u64);
    Ok(ReplicaFetchedItem {
        handle,
        link_id: link,
        meta,
        sort_key,
        body: Some(ReplicaFetchedBody::Persisted { hash, size }),
        // The revision this body corresponds to, so a later edit can be pushed
        // conditionally on it. `None` on immutable content.
        revision,
    })
}

/// Fetches a batch of bodies in one `UID FETCH … (UID BODY.PEEK[])` command, each
/// streamed straight into its own blob (bounded memory), returning a fetched item
/// per message. The message's UID (from its FETCH line) keys it back to its
/// handle, so out-of-order server responses still route correctly. On any batch
/// error — a server that ordered `UID` after the body, or a transient failure —
/// it falls back to per-message fetches; content-addressing makes the partial
/// retry idempotent. Ticks `on_body` per body. Runs on one connection (one
/// worker), so no cross-thread sharing inside.
pub(crate) fn hydrate_batch(
    kind: Kind,
    client: &mut Client,
    collection: &str,
    handles: &[ReplicaHandle],
    blob: &PimdirBlobs,
    on_body: Option<&(dyn Fn() + Sync)>,
) -> Result<Vec<ReplicaFetchedItem>> {
    let ids: Vec<&str> = handles.iter().map(|h| h.as_str()).collect();
    let mut items: Vec<ReplicaFetchedItem> = Vec::with_capacity(handles.len());

    let batched = client.fetch_bodies(
        collection,
        &ids,
        |_id| {
            blob.writer()
                .map(|writer| HydrateSink::new(writer, blob.hasher()))
        },
        |id, revision, sink: HydrateSink| {
            let (hash, size, header) = sink.finish().map_err(io::Error::other)?;
            let (link, meta, sort_key) = kind.parse_body(&header, size as u64);
            items.push(ReplicaFetchedItem {
                handle: ReplicaHandle::from(id),
                link_id: link,
                meta,
                sort_key,
                body: Some(ReplicaFetchedBody::Persisted { hash, size }),
                revision: revision.map(str::to_string),
            });
            if let Some(cb) = on_body {
                cb();
            }
            Ok(())
        },
    );

    match batched {
        Ok(()) => Ok(items),
        Err(err) => {
            warn!("batched fetch `{collection}` failed ({err:#}); falling back to per-message");
            // Discard any partial batch items and refetch each body; the blobs are
            // content-addressed, so re-committing is idempotent.
            items.clear();
            let blob = blob.clone();
            for handle in handles {
                items.push(fetch_one_full(
                    kind,
                    client,
                    collection,
                    handle.clone(),
                    &blob,
                )?);
                if let Some(cb) = on_body {
                    cb();
                }
            }
            Ok(items)
        }
    }
}

impl PimRemote<'_> {
    /// Appends a stored body as a genuine new member (a cross-side copy:
    /// the two sides are different servers, so no server-side copy is
    /// possible; the deduped body is uploaded).
    fn append(
        &mut self,
        collection: &str,
        handle: ReplicaHandle,
        flags: &ReplicaFlags,
        object: Option<ReplicaHash>,
        message_id: Option<&str>,
    ) -> ReplicaPushResult {
        let Some(hash) = object else {
            warn!(
                "append with no stored body for `{}`, rejecting",
                handle.as_str()
            );
            return rejected_bare(handle);
        };

        // NOTE: stream the stored body straight to the socket; the body is never
        // read into memory whole (bounded-memory transfer).
        let reader = match self.blob.reader(&hash) {
            Ok(Some(file)) => file,
            Ok(None) => {
                warn!("append body `{}` missing from blob store", hash.as_str());
                return rejected_bare(handle);
            }
            Err(err) => {
                warn!("append body read error: {err:#}");
                return rejected_bare(handle);
            }
        };
        let len = match reader.metadata() {
            Ok(meta) => meta.len() as usize,
            Err(err) => {
                warn!("append body stat error: {err:#}");
                return rejected_bare(handle);
            }
        };

        let item_flags = to_item_flags(flags);
        match self
            .pool
            .primary()
            .add_item_stream(collection, &item_flags, reader, len, message_id)
        {
            Ok(written) => ReplicaPushResult {
                handle,
                outcome: ReplicaPushOutcome::Accepted,
                assigned: Some(ReplicaHandle::from(written.id)),
                // The revision the remote now holds, so the next merge
                // compares against what it actually wrote rather than
                // re-fetching to find out. `None` on immutable content.
                revision: written.revision,
            },
            Err(err) => {
                warn!("append to `{collection}` error: {err:#}");
                rejected_bare(handle)
            }
        }
    }

    /// Replaces an item's body in place, conditionally on the last-synced
    /// revision.
    ///
    /// A refusal is reported as [`ReplicaPushOutcome::Rejected`], **not** as an
    /// error: rejection is the expected outcome when the remote moved since the
    /// base, and it is what makes io-replica re-merge and mark the placement
    /// conflicted instead of clobbering the remote body. An error would abort
    /// the whole batch.
    fn update(
        &mut self,
        collection: &str,
        handle: ReplicaHandle,
        object: ReplicaHash,
        if_match: Option<&str>,
    ) -> ReplicaPushResult {
        // The edited body streams straight from the blob store, as an append
        // does: bounded memory regardless of size.
        let reader = match self.blob.reader(&object) {
            Ok(Some(file)) => file,
            Ok(None) => {
                warn!("update body `{}` missing from blob store", object.as_str());
                return rejected_bare(handle);
            }
            Err(err) => {
                warn!("update body read error: {err:#}");
                return rejected_bare(handle);
            }
        };
        let len = match reader.metadata() {
            Ok(meta) => meta.len() as usize,
            Err(err) => {
                warn!("update body stat error: {err:#}");
                return rejected_bare(handle);
            }
        };

        match self.pool.primary().update_item_stream(
            collection,
            handle.as_str(),
            reader,
            len,
            if_match,
        ) {
            Ok(revision) => ReplicaPushResult {
                handle,
                outcome: ReplicaPushOutcome::Accepted,
                assigned: None,
                revision,
            },
            Err(err) => {
                warn!("update `{}` in `{collection}` rejected: {err:#}", handle.0);
                rejected_bare(handle)
            }
        }
    }
}

fn accepted(handle: ReplicaHandle, assigned: Option<ReplicaHandle>) -> ReplicaPushResult {
    ReplicaPushResult {
        handle,
        outcome: ReplicaPushOutcome::Accepted,
        assigned,
        revision: None,
    }
}

fn rejected(handle: ReplicaHandle, what: &str, err: anyhow::Error) -> ReplicaPushResult {
    warn!("{what} `{}` rejected: {err:#}", handle.as_str());
    rejected_bare(handle)
}

fn rejected_bare(handle: ReplicaHandle) -> ReplicaPushResult {
    ReplicaPushResult {
        handle,
        outcome: ReplicaPushOutcome::Rejected,
        assigned: None,
        revision: None,
    }
}

/// Cap on captured header bytes; bounds memory if a message has no header/body
/// boundary (defensive — real headers are a few KB).
const HEADER_CAP: usize = 256 * 1024;

/// A [`Write`] sink for a streaming Full fetch. It tees each chunk into the blob
/// store, folds it into the content hash, and captures the header prefix (up to
/// the blank line) so the link id and summary parse without a second pass — so
/// the whole body never sits in memory.
struct HydrateSink {
    writer: PimdirBlobWriter,
    hasher: PimdirHasher,
    header: Vec<u8>,
    header_done: bool,
}

impl HydrateSink {
    /// `hasher` comes from the store's blob handle, so a body is named by
    /// the algorithm the store records (pimdir SPEC §5) and dedups against
    /// what any other consumer of the same store wrote.
    fn new(writer: PimdirBlobWriter, hasher: PimdirHasher) -> Self {
        Self {
            writer,
            hasher,
            header: Vec::new(),
            header_done: false,
        }
    }

    /// Commits the streamed blob under its computed hash and returns
    /// `(hash, size, captured header bytes)`.
    fn finish(self) -> Result<(ReplicaHash, usize, Vec<u8>)> {
        let hash = self.hasher.finish();
        let size = self.writer.commit(&hash)? as usize;
        Ok((hash, size, self.header))
    }
}

impl Write for HydrateSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write_all(buf)?;
        self.hasher.update(buf);
        if !self.header_done {
            // Re-scan a few bytes back so a boundary split across chunks is
            // still found.
            let from = self.header.len().saturating_sub(3);
            self.header.extend_from_slice(buf);
            if let Some(end) = header_boundary(&self.header[from..]) {
                self.header.truncate(from + end);
                self.header_done = true;
            } else if self.header.len() >= HEADER_CAP {
                self.header.truncate(HEADER_CAP);
                self.header_done = true;
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

/// The byte offset just past the header/body boundary (`\r\n\r\n`, or a bare
/// `\n\n`) in `buf`, or `None` when it is not present.
fn header_boundary(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| buf.windows(2).position(|w| w == b"\n\n").map(|i| i + 2))
}
