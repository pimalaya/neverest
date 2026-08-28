//! [`io_replica::client::ReplicaRemote`] backed by one [`Client`], one side of
//! the sync.
//!
//! `enumerate` reads the collection's handle and flag spine incrementally,
//! passing the stored cursor down opaquely so the backend decides whether it
//! can answer a delta or owes a full snapshot. io-replica derives what vanished
//! by diffing a complete snapshot against the stored placements. `fetch`
//! resolves the link id and the summary through [`Kind`], at the cheap
//! server-side tier when the kind has one and from the raw body otherwise.
//! `push` maps the four [`ReplicaChangeKind`] variants onto the client's flag,
//! delete, move, append and update calls.
//!
//! Everything kind-specific (the link id, the summary, the sort key) lives in
//! [`crate::kind`] and is resolved once per side from
//! [`Client::media_type`](crate::client::Client::media_type). Bodies are
//! content-addressed by the store's own hash ([`PimdirBlobs::hasher`]). A
//! mutable-content kind carries a revision, so its writes are conditional on
//! the last-synced one; an immutable one leaves it `None` and never conflicts.

use std::{
    cmp::Reverse,
    collections::{BTreeSet, HashMap},
    io::{self, Write},
    mem,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use anyhow::{Context, Result, bail};
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

#[cfg(feature = "dav")]
use crate::dav::client::is_duplicate_uid;
use crate::{
    client::{Client, Pool},
    item::{
        flag::{Flag, FlagOp},
        summary::ItemSummary,
    },
    kind::{Kind, LinkId},
};

/// No DAV backend is compiled in, so no write can be refused with the
/// `no-uid-conflict` precondition: it is the only refusal named that way, mail
/// carrying no identity a server enforces.
#[cfg(not(feature = "dav"))]
fn is_duplicate_uid(_err: &anyhow::Error) -> bool {
    false
}

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
    /// The hub namespace this source binds into, stripped off a collection id
    /// before it reaches the wire. See [`PimRemote::wire_name`].
    namespace: String,
    /// Called once per body streamed at the `Full` tier, so the driver can tick
    /// a per-item progress counter. `Sync` because the fetch pool calls it from
    /// several worker threads at once.
    on_body: Option<&'a (dyn Fn() + Sync)>,
    /// Handle to body octet size, from the store's already-fetched envelope
    /// meta, so no round trip. When present, `Full` fetches are ordered
    /// largest-first so the heavy items go up front and the progress counter
    /// accelerates to a smooth finish rather than stalling on a big body that
    /// happened to land last.
    sizes: HashMap<String, u64>,
    /// What this side is known to hold, so a create answered with a handle it
    /// already holds is caught before it becomes a second binding.
    held: HeldHandles,
    /// The creates this side refused because it already holds the item's
    /// identity, drained into the report by the driver, which names the
    /// source. Filled in push order, one entry per refused create, and never
    /// deduplicated: two copies refused are two lines.
    refused: Vec<RefusedCreate>,
}

/// The handles one side is known to hold, per collection: everything its
/// enumerations named this run, less what they reported vanished, plus every
/// handle a create was assigned.
///
/// It is a floor rather than a proof of what the side holds. A full
/// enumeration (a DAV collection with no `sync-collection`, an IMAP resync)
/// makes it the whole collection; an incremental one narrows it to what
/// changed plus this run's own creates. That is enough for what it guards,
/// a server answering a create with a resource it already had.
#[derive(Default)]
struct HeldHandles(HashMap<String, BTreeSet<String>>);

impl HeldHandles {
    /// Folds one enumeration of `collection` in.
    fn remember(
        &mut self,
        collection: &str,
        items: &[ReplicaRemoteItem],
        vanished: &[ReplicaHandle],
    ) {
        let held = self.0.entry(collection.to_string()).or_default();
        held.extend(items.iter().map(|item| item.handle.0.clone()));
        for handle in vanished {
            held.remove(handle.as_str());
        }
    }

    /// Claims `handle` for a create, answering whether it was free. A handle
    /// the side already holds is not a new member: the server answered the
    /// create with a resource it had, and binding the item to it would leave
    /// two items on one handle.
    fn claim(&mut self, collection: &str, handle: &str) -> bool {
        self.0
            .entry(collection.to_string())
            .or_default()
            .insert(handle.to_string())
    }
}

/// One create a side refused with the CalDAV or CardDAV `no-uid-conflict`
/// precondition, as the remote knows it.
///
/// It carries the collection as the wire names it and the identity the two
/// copies share; the source's name is the driver's to add, this seam knowing
/// the namespace a collection binds into rather than the name the report
/// speaks of it by.
pub struct RefusedCreate {
    /// The collection the create was refused in, as the server names it.
    pub collection: String,
    /// The identity the refused copy shares with the resource already there.
    pub uid: String,
}

impl<'a> PimRemote<'a> {
    pub fn new(pool: &'a mut Pool, blob: PimdirBlobs, namespace: impl Into<String>) -> Self {
        let kind = resolve_kind(pool);
        Self {
            kind,
            pool,
            blob,
            namespace: namespace.into(),
            on_body: None,
            sizes: HashMap::new(),
            held: HeldHandles::default(),
            refused: Vec::new(),
        }
    }

    /// Like [`new`](Self::new) but ticks `on_body` after each `Full` body
    /// streams, and orders the `Full` fetch largest-first using `sizes`, taken
    /// from the store's envelope meta. Pass an empty map to keep handle order.
    pub fn with_progress(
        pool: &'a mut Pool,
        blob: PimdirBlobs,
        namespace: impl Into<String>,
        on_body: &'a (dyn Fn() + Sync),
        sizes: HashMap<String, u64>,
    ) -> Self {
        let kind = resolve_kind(pool);
        Self {
            kind,
            pool,
            blob,
            namespace: namespace.into(),
            on_body: Some(on_body),
            sizes,
            held: HeldHandles::default(),
            refused: Vec::new(),
        }
    }

    /// [`wire_name`] against this side's namespace.
    fn wire_name<'n>(&self, collection: &'n str) -> &'n str {
        wire_name(&self.namespace, collection)
    }

    /// The creates this side refused because it already holds the identity,
    /// taken off the remote so the driver can name them in the report.
    pub fn take_refused(&mut self) -> Vec<RefusedCreate> {
        mem::take(&mut self.refused)
    }
}

/// The name the backend knows a hub collection by.
///
/// A hub collection id is `<namespace>/<name>`, which is what keeps a mailbox
/// and an address book both called `Default` apart in one store, and what keeps
/// two providers cached side by side from meeting. The server knows nothing of
/// that: this is the seam where the id becomes a name again, and every wire call
/// goes through it, including the ones the driver's fetch pool makes on its own
/// connections rather than through [`PimRemote`]. A leak reaches the server as a
/// collection it has no name for, which IMAP rejects outright (`/` is not legal
/// in a mailbox name on a server whose delimiter is `.`) and a path-addressed
/// backend would look up under a directory that does not exist.
///
/// A name that merely starts with the namespace keeps its own spelling: only the
/// `<namespace>/` prefix is a namespace.
pub(crate) fn wire_name<'n>(namespace: &str, collection: &'n str) -> &'n str {
    collection
        .strip_prefix(namespace)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(collection)
}

/// The [`Kind`] a pool's backend syncs.
///
/// The driver refuses an unknown or cross-kind media type before opening any
/// remote, so this cannot legitimately fail. Falling back to [`Kind::Mail`]
/// rather than panicking keeps a construction path that takes no `Result`
/// honest, and the warning names the media type if it ever does.
pub(crate) fn resolve_kind(pool: &mut Pool) -> Kind {
    let media_type = pool.primary().media_type();
    Kind::from_media_type(media_type).unwrap_or_else(|| {
        warn!("unknown media type {media_type}, deriving link ids as mail");
        Kind::Mail
    })
}

/// How many bodies to request per batched fetch. Larger cuts round trips but
/// coarsens the per-batch retry unit and the command size.
pub(crate) const BATCH_SIZE: usize = 64;

/// Maps shared flags to protocol-neutral offline flag strings, their raw wire
/// spelling. A single side is internally consistent, and both sides run the
/// same normalization for the system flags that matter.
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
        let collection = self.wire_name(collection.as_str());
        let cursor = cursor.as_ref().map(|c| c.0.as_slice());
        let enumeration = self
            .pool
            .primary()
            .enumerate(collection, cursor)
            .with_context(|| format!("Enumerate {collection} error"))?;
        let items: Vec<ReplicaRemoteItem> = enumeration
            .items
            .into_iter()
            .map(|entry| ReplicaRemoteItem {
                handle: ReplicaHandle::from(entry.id),
                flags: to_offline_flags(&entry.flags),
                revision: entry.revision,
            })
            .collect();
        let vanished: Vec<ReplicaHandle> = enumeration
            .vanished
            .into_iter()
            .map(ReplicaHandle::from)
            .collect();
        self.held.remember(collection, &items, &vanished);
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
        let collection = self.wire_name(collection.as_str());

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
        let collection = self.wire_name(collection.as_str()).to_string();
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
                ReplicaChangeKind::Remove {
                    handle,
                    to,
                    link_id: _,
                    if_match,
                } => match to {
                    Some(target) => {
                        let dest = wire_name(&self.namespace, target.as_str()).to_string();
                        match self
                            .pool
                            .primary()
                            .move_items(&collection, &dest, &[handle.as_str()])
                        {
                            Ok(()) => accepted(handle, None),
                            Err(err) => rejected(handle, "move item", err),
                        }
                    }
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
                    let link = link_id
                        .as_ref()
                        .map(|link| self.kind.split_link_id(link))
                        .unwrap_or_default();
                    self.append(&collection, handle, &flags, object, link)
                }
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
    /// Meta tier: a targeted summary fetch of just the requested handles, with
    /// no bodies and no whole-collection sweep, so the link ids and summaries
    /// resolve cheaply and the cost scales with the change.
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
            .with_context(|| format!("Fetch envelopes {collection} error"))?;
        let by_id: HashMap<&str, &ItemSummary> =
            envelopes.iter().map(|e| (e.id.as_str(), e)).collect();

        let mut items = Vec::with_capacity(handles.len());
        for handle in handles {
            let Some(env) = by_id.get(handle.as_str()) else {
                continue;
            };
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

    /// Full tier: every body streamed straight into the blob store, never held
    /// whole, and fetched in batches rather than one command per item, so N
    /// bodies cost about N/[`BATCH_SIZE`] round trips. Batches are work-stolen
    /// across a bounded pool of connections, and the engine serialises the index
    /// write afterwards.
    fn fetch_full(
        &mut self,
        collection: &str,
        mut handles: Vec<ReplicaHandle>,
    ) -> Result<Vec<ReplicaFetchedItem>> {
        if handles.is_empty() {
            return Ok(Vec::new());
        }
        if self.sizes.is_empty() {
            handles.sort_by_key(|h| h.as_str().parse::<u64>().unwrap_or(u64::MAX));
        } else {
            handles.sort_by_key(|h| Reverse(self.sizes.get(h.as_str()).copied().unwrap_or(0)));
        }

        let total = handles.len();
        let target = self.pool.max().min(total);
        let batches: Vec<Vec<ReplicaHandle>> = handles
            .chunks(BATCH_SIZE)
            .map(<[ReplicaHandle]>::to_vec)
            .collect();

        if target <= 1 {
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
    /// connections: a shared queue of batches, each worker draining it on its
    /// own connection. Work-stealing balances the load with no size probe, a
    /// worker with heavy batches naturally taking fewer.
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

        let kind = self.kind;
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
        .with_context(|| format!("Stream item {} in {collection} error", handle.as_str()))?;
    let (hash, size, header) = sink
        .finish()
        .with_context(|| format!("Commit body {} in {collection} error", handle.as_str()))?;

    // No kind this syncs has an empty body: a message carries headers and a
    // card carries at least its BEGIN and END lines. Storing one anyway gives
    // it a link id that is the digest of nothing, so every empty body a server
    // hands back resolves to that one identity, and each after the first is
    // filed as another copy of it.
    if size == 0 {
        bail!(
            "Server returned an empty body for {} in {collection}",
            handle.as_str(),
        );
    }

    let (link, meta, sort_key) = kind.parse_body(&header, size as u64);
    Ok(ReplicaFetchedItem {
        handle,
        link_id: link,
        meta,
        sort_key,
        body: Some(ReplicaFetchedBody::Persisted { hash, size }),
        revision,
    })
}

/// Fetches a batch of bodies in one command, each streamed straight into its own
/// blob so memory stays bounded, returning one fetched item per body. The
/// handle the server echoes keys each body back, so out-of-order responses
/// still route correctly. A batch error, and a batch answering for fewer
/// members than it was asked about, both fall back to per-item fetches, which
/// content-addressing makes idempotent. Ticks `on_body` per body, and runs on
/// one connection so nothing inside is shared across threads.
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
        Ok(()) => {
            // A batch answering for fewer members than it was asked about is
            // not a batch that succeeded: the engine would record nothing for
            // the rest and ask for them again on every later run. A CardDAV
            // server was found doing exactly that, returning the ETag of each
            // card and its body as `404 Not Found`.
            let fetched: BTreeSet<String> = items
                .iter()
                .map(|item| item.handle.as_str().to_owned())
                .collect();
            let missing: Vec<ReplicaHandle> = handles
                .iter()
                .filter(|handle| !fetched.contains(handle.as_str()))
                .cloned()
                .collect();

            if !missing.is_empty() {
                warn!(
                    "batched fetch {collection} returned {} of {} bodies; \
                     fetching the rest one by one",
                    items.len(),
                    handles.len(),
                );

                let blob = blob.clone();
                for handle in missing {
                    items.push(fetch_one_full(kind, client, collection, handle, &blob)?);
                    if let Some(cb) = on_body {
                        cb();
                    }
                }
            }

            Ok(items)
        }
        Err(err) => {
            warn!("batched fetch {collection} failed ({err:#}); falling back to per-item");
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
    /// Appends a stored body as a genuine new member. The two sides are
    /// different servers, so no server-side copy is possible and the deduped
    /// body is uploaded.
    ///
    /// Two things about the answer are checked before it becomes a binding.
    ///
    /// A server may refuse the create outright because it already holds the
    /// item's `UID` (RFC 4791 §5.3.2, RFC 6352 §6.3.2), which is reported as
    /// what it is rather than as a status, and comes back every run until that
    /// side stops holding the identity twice.
    ///
    /// A server may instead answer a create by *updating* the resource that
    /// already holds the `UID` and handing back its href, which RFC 6352
    /// §6.3.2 forbids and nothing here can prevent, so it is caught on the way
    /// back: an assigned handle this side is already known to hold is recorded
    /// as a rejected push and never bound. The engine binds one handle per
    /// item per source, and two items pointing at one handle make the next
    /// enumeration read one of them as vanished, which propagates a delete of
    /// a resource nobody removed.
    ///
    /// What this side is known to hold is a floor rather than a proof: a full
    /// enumeration (a DAV collection with no `sync-collection`, an IMAP
    /// resync) makes it the whole collection, an incremental one narrows it to
    /// what changed plus this run's own creates.
    fn append(
        &mut self,
        collection: &str,
        handle: ReplicaHandle,
        flags: &ReplicaFlags,
        object: Option<ReplicaHash>,
        link: LinkId<'_>,
    ) -> ReplicaPushResult {
        let Some(hash) = object else {
            warn!(
                "append with no stored body for {}, rejecting",
                handle.as_str()
            );
            return rejected_bare(handle);
        };

        let reader = match self.blob.reader(&hash) {
            Ok(Some(file)) => file,
            Ok(None) => {
                warn!("append body {} missing from blob store", hash.as_str());
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
        let written =
            self.pool
                .primary()
                .add_item_stream(collection, &item_flags, reader, len, link);

        let written = match written {
            Ok(written) => written,
            Err(err) => {
                if is_duplicate_uid(&err) {
                    let uid = link.hint.unwrap_or(handle.as_str());
                    warn!("append to {collection} refused: it already holds UID {uid}");
                    self.refused.push(RefusedCreate {
                        collection: collection.to_string(),
                        uid: uid.to_string(),
                    });
                } else {
                    warn!("append to {collection} error: {err:#}");
                }
                return rejected_bare(handle);
            }
        };

        let assigned = ReplicaHandle::from(written.id);
        if !self.held.claim(collection, assigned.as_str()) {
            warn!(
                "append to {collection} was answered with {}, which it already holds; rejecting",
                assigned.as_str(),
            );
            return rejected_bare(handle);
        }

        ReplicaPushResult {
            handle,
            outcome: ReplicaPushOutcome::Accepted,
            assigned: Some(assigned),
            revision: written.revision,
        }
    }

    /// Replaces an item's body in place, conditionally on the last-synced
    /// revision.
    ///
    /// A refusal is reported as [`ReplicaPushOutcome::Rejected`] rather than as
    /// an error: rejection is the expected outcome when the remote moved since
    /// the base, and it is what makes io-replica re-merge and mark the
    /// placement conflicted instead of clobbering the remote body. An error
    /// would abort the whole batch.
    fn update(
        &mut self,
        collection: &str,
        handle: ReplicaHandle,
        object: ReplicaHash,
        if_match: Option<&str>,
    ) -> ReplicaPushResult {
        let reader = match self.blob.reader(&object) {
            Ok(Some(file)) => file,
            Ok(None) => {
                warn!("update body {} missing from blob store", object.as_str());
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
                warn!("update {} in {collection} rejected: {err:#}", handle.0);
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
    warn!("{what} {} rejected: {err:#}", handle.as_str());
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

/// Cap on captured header bytes, bounding memory if an item carries no header
/// and body boundary at all.
const HEADER_CAP: usize = 256 * 1024;

/// A [`Write`] sink for a streaming `Full` fetch. It tees each chunk into the
/// blob store, folds it into the content hash, and captures the header prefix
/// up to the blank line so the link id and summary parse without a second pass.
/// The whole body never sits in memory.
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

/// The byte offset just past the header and body boundary in `buf`, or `None`
/// when it is not present.
fn header_boundary(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| buf.windows(2).position(|w| w == b"\n\n").map(|i| i + 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every wire call goes through this seam, the driver's own fetch pool
    /// included. A hub id that reaches a server is rejected outright on IMAP,
    /// whose mailbox names cannot hold the `/` a namespace prefix ends on.
    #[test]
    fn a_hub_id_becomes_the_name_its_server_knows() {
        assert_eq!(
            wire_name("imap", "imap/Archives.Charlie"),
            "Archives.Charlie"
        );
        assert_eq!(wire_name("mail", "mail/INBOX"), "INBOX");

        // An IMAP hierarchy survives: only the first segment is the namespace.
        assert_eq!(wire_name("mail", "mail/Archive/2026"), "Archive/2026");

        // A name that merely starts with the namespace keeps its spelling.
        assert_eq!(wire_name("mail", "mailbox/INBOX"), "mailbox/INBOX");

        // An id from another namespace is left whole rather than mangled.
        assert_eq!(wire_name("cards", "mail/INBOX"), "mail/INBOX");
    }

    /// One enumerated member, the shape [`HeldHandles::remember`] folds in.
    fn member(handle: &str) -> ReplicaRemoteItem {
        ReplicaRemoteItem {
            handle: ReplicaHandle(handle.into()),
            flags: ReplicaFlags::default(),
            revision: None,
        }
    }

    /// A server that answers a create by updating the resource already
    /// holding the `UID` hands back an href this side is known to hold. Two
    /// items on one handle make the next enumeration read one of them as
    /// vanished, which propagates a delete of a resource nobody removed, so
    /// the create is rejected instead of bound.
    #[test]
    fn a_create_answered_with_a_handle_the_side_holds_is_refused() {
        let mut held = HeldHandles::default();
        held.remember("agenda", &[member("event-1.ics")], &[]);

        assert!(
            !held.claim("agenda", "event-1.ics"),
            "the server answered with a resource it already had",
        );
        assert!(
            held.claim("agenda", "event-2.ics"),
            "a genuinely new member is bound",
        );
        assert!(
            !held.claim("agenda", "event-2.ics"),
            "and is held from then on, so a second create onto it is refused too",
        );

        assert!(
            held.claim("contacts", "event-1.ics"),
            "handles are per collection, two collections never colliding",
        );
    }

    /// A member the server reported gone is free again: its href may be
    /// reused, and refusing a create onto a resource nobody holds would keep
    /// an item unwritable for the rest of the run.
    #[test]
    fn a_vanished_handle_stops_being_held() {
        let mut held = HeldHandles::default();
        held.remember("agenda", &[member("event-1.ics")], &[]);
        held.remember("agenda", &[], &[ReplicaHandle("event-1.ics".into())]);

        assert!(held.claim("agenda", "event-1.ics"));
    }
}
