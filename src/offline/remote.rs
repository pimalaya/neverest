//! # One side's remote seam
//!
//! [`io_pimdir::remote::PimdirRemote`] backed by one [`Client`].
//!
//! `enumerate` passes the stored cursor down opaquely, so the backend decides
//! whether it can answer a delta or owes a full snapshot. `fetch` resolves the
//! link id and the summary through [`Kind`]. `push` maps the four
//! [`PimdirChangeKind`] variants onto the client's calls.
//!
//! Everything kind-specific lives in [`crate::kind`], resolved once per side
//! from [`Client::media_type`](crate::client::Client::media_type). A
//! mutable-content kind carries a revision, so its writes are conditional on
//! the last-synced one; an immutable one leaves it `None` and never conflicts.

use std::{
    cmp::Reverse,
    collections::{BTreeSet, HashMap, HashSet},
    fmt,
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
use io_pimdir::{
    change::{PimdirChange, PimdirChangeKind},
    client::blobs::{PimdirBlobWriter, PimdirBlobs},
    collection::{PimdirCheckpoint, PimdirCollectionId, PimdirCursor, PimdirScope},
    hash::PimdirHasher,
    load::PimdirLoaded,
    object::PimdirHash,
    placement::{PimdirFlags, PimdirHandle, PimdirLinkId, PimdirOrigin, PimdirSortKey},
    remote::{
        PimdirEnumerate, PimdirEnumerated, PimdirFetchedBody, PimdirFetchedItem, PimdirPushOutcome,
        PimdirPushResult, PimdirRemote, PimdirRemoteItem, PimdirRemoteMeta, PimdirRemoteSnapshot,
        PimdirTier,
    },
    summary::{PimdirDerivation, PimdirSummary},
};
use log::warn;

#[cfg(feature = "dav")]
use crate::dav::client::is_duplicate_uid;
use crate::{
    client::{Client, EnumEntry, Held, Listed, Pool, WrittenItem},
    item::{
        flag::{Flag, FlagOp},
        summary::ItemSummary,
    },
    kind::{Kind, LinkId},
};

/// No DAV backend, so nothing can be refused with `no-uid-conflict`.
#[cfg(not(feature = "dav"))]
fn is_duplicate_uid(_err: &anyhow::Error) -> bool {
    false
}

/// One side's remote seam over its connection [`Pool`].
///
/// Sequential verbs run on the pool's primary connection; a `Full` fetch
/// borrows several at once. The pool is persistent, so the extra connections'
/// auth is paid once for the whole run rather than per batch.
pub struct PimRemote<'a> {
    /// The kind this side syncs, resolved once from [`Client::media_type`].
    ///
    /// The driver has already refused an unknown or mismatched media type
    /// before any remote is built.
    kind: Kind,
    pool: &'a mut Pool,
    blob: PimdirBlobs,
    /// The hub namespace, stripped off before any wire call. See [`wire_name`].
    namespace: String,
    /// Ticked once per `Full` body, for the driver's progress counter.
    ///
    /// `Sync` because the fetch pool calls it from several workers at once.
    on_body: Option<&'a (dyn Fn() + Sync)>,
    /// Handle to body octet size, from the store's summary, so no round trip.
    ///
    /// When present, `Full` fetches run largest-first, so progress accelerates
    /// to a smooth finish rather than stalling on a big body that landed last.
    sizes: HashMap<String, u64>,
    /// What this side is known to hold.
    ///
    /// A create answered with a handle it already holds is caught before it
    /// becomes a second binding.
    held: HeldHandles,
    /// The creates this side refused because it already holds the identity.
    ///
    /// One entry per refused create, in push order: two copies refused are two
    /// lines, which is what the handle on each entry keeps true once the driver
    /// drops the repeats a convergence loop collects.
    refused: Vec<RefusedCreate>,
    /// The writes this side would not take, one entry each, in push order.
    ///
    /// Drained into the report by the driver, so a run says what it could not
    /// deliver rather than counting it among the hunks it applied.
    rejected: Vec<RejectedPush>,
    /// What the store binds of one hub collection, which names a listed
    /// member the listing read no meta for.
    bound: Option<(String, Bound)>,
    /// Whether a member named by its body has the body fetched as its page
    /// is listed; false on a dry run, which downloads nothing.
    bodies: bool,
    /// Whether the connector's checkpoint is bound to its scope, read once
    /// from [`Client::scope_bound`].
    scope_bound: bool,
}

/// What the store binds of a collection for one source.
#[derive(Default)]
pub struct Bound {
    /// The bound handles.
    handles: HashSet<String>,
    /// The name of each, by handle.
    metas: HashMap<String, BoundMeta>,
    /// The checkpoint the store holds.
    checkpoint: Option<Vec<u8>>,
}

impl Bound {
    /// What a load of the collection binds: every based placement that
    /// carries its link id.
    pub fn from_loaded(loaded: PimdirLoaded) -> Self {
        let metas: HashMap<String, BoundMeta> = loaded
            .placements
            .into_iter()
            .filter_map(|placement| {
                let base = placement.base?;
                let link_id = placement.link_id?;
                let meta = BoundMeta {
                    revision: base.revision,
                    link_id,
                    sort_key: placement.sort_key,
                };
                Some((placement.handle.0, meta))
            })
            .collect();

        Self {
            handles: metas.keys().cloned().collect(),
            metas,
            checkpoint: loaded.checkpoint.map(|checkpoint| checkpoint.0),
        }
    }
}

/// How the store names one bound member.
struct BoundMeta {
    revision: Option<String>,
    link_id: PimdirLinkId,
    sort_key: PimdirSortKey,
}

impl BoundMeta {
    /// The meta naming the member as the store does, its summary kept.
    fn meta(&self) -> PimdirRemoteMeta {
        PimdirRemoteMeta {
            link_id: self.link_id.clone(),
            summary: None,
            sort_key: self.sort_key.clone(),
            body: None,
        }
    }
}

/// Drops the members of a page whose `Date` lies outside `scope` (pimdir
/// SYNC §5): a connector narrows its listing by the provider's reception
/// date to a superset at most, and the `Date` decides, a member with no
/// usable date being in every scope.
fn keep_in_scope(items: &mut Vec<PimdirRemoteItem>, scope: &PimdirScope) {
    if !scope.is_unbounded() {
        items.retain(|item| scope.contains(listed_date(&item.meta)));
    }
}

/// The date a listed member is in scope by: its summary's, else its sort
/// key, which a mail sort key is (pimdir STORAGE Annex A.1).
fn listed_date(meta: &PimdirRemoteMeta) -> Option<&str> {
    match &meta.summary {
        Some(PimdirSummary::Mail(summary)) => summary.date.as_deref(),
        _ => Some(meta.sort_key.as_str()).filter(|key| !key.is_empty()),
    }
}

/// The handles one side is known to hold, per collection.
///
/// A floor rather than a proof: a full enumeration makes it the whole
/// collection, an incremental one only what changed plus this run's creates.
/// Enough for what it guards: a server answering a create with a held href.
#[derive(Default)]
struct HeldHandles(HashMap<String, BTreeSet<String>>);

impl HeldHandles {
    /// Folds one enumeration of `collection` in.
    fn remember(
        &mut self,
        collection: &str,
        items: &[PimdirRemoteItem],
        vanished: &[PimdirHandle],
    ) {
        let held = self.0.entry(collection.to_string()).or_default();
        held.extend(items.iter().map(|item| item.handle.0.clone()));
        for handle in vanished {
            held.remove(handle.as_str());
        }
    }

    /// Claims `handle` for a create, answering whether it was free.
    ///
    /// A handle the side already holds is not a new member: the server answered
    /// the create with a resource it had, and binding the item to it would
    /// leave two items on one handle.
    fn claim(&mut self, collection: &str, handle: &str) -> bool {
        self.0
            .entry(collection.to_string())
            .or_default()
            .insert(handle.to_string())
    }
}

/// One create a side refused with the `no-uid-conflict` precondition.
///
/// The source's name is the driver's to add, this seam knowing the namespace a
/// collection binds into rather than the name the report speaks of it by.
pub struct RefusedCreate {
    /// The collection the create was refused in, as the server names it.
    pub collection: String,
    /// The identity the refused copy shares with the resource already there.
    pub uid: String,
    /// The handle the refused copy was being appended under.
    ///
    /// A key rather than something the report says: it tells two copies of one
    /// identity apart from the same copy refused again on a later pass, the
    /// copies sharing everything a person would act on.
    pub handle: String,
}

/// One write that did not land, as the remote knows it.
///
/// Both halves of a rejection: what a server answered no to, and what never
/// reached one for want of a body. Both leave the change in the store for the
/// next run. A create refused with `no-uid-conflict` has [`RefusedCreate`].
pub struct RejectedPush {
    /// The collection the write was going into, as the server names it.
    pub collection: String,
    /// The item's handle on this side.
    pub handle: String,
    /// What the run tried: `update`, `append`, `copy`, `delete`, `move`,
    /// `set flags`.
    pub action: &'static str,
    /// Why it did not land, as the backend put it.
    pub reason: String,
}

impl<'a> PimRemote<'a> {
    /// Binds a remote to the pool it calls through and the namespace it strips.
    pub fn new(pool: &'a mut Pool, blob: PimdirBlobs, namespace: impl Into<String>) -> Self {
        let kind = resolve_kind(pool);
        let scope_bound = pool.primary().scope_bound();
        Self {
            kind,
            pool,
            blob,
            namespace: namespace.into(),
            on_body: None,
            sizes: HashMap::new(),
            held: HeldHandles::default(),
            refused: Vec::new(),
            rejected: Vec::new(),
            bound: None,
            bodies: true,
            scope_bound,
        }
    }

    /// Like [`new`](Self::new), but ticking `on_body` per streamed `Full` body.
    ///
    /// The `Full` fetch runs largest-first by `sizes`, taken from the store's
    /// mail summary. An empty map keeps handle order.
    pub fn with_progress(
        pool: &'a mut Pool,
        blob: PimdirBlobs,
        namespace: impl Into<String>,
        on_body: &'a (dyn Fn() + Sync),
        sizes: HashMap<String, u64>,
    ) -> Self {
        let kind = resolve_kind(pool);
        let scope_bound = pool.primary().scope_bound();
        Self {
            kind,
            pool,
            blob,
            namespace: namespace.into(),
            on_body: Some(on_body),
            sizes,
            held: HeldHandles::default(),
            refused: Vec::new(),
            rejected: Vec::new(),
            bound: None,
            bodies: true,
            scope_bound,
        }
    }

    /// Names the members of hub collection `collection` the store binds
    /// the way it binds them, so a listing reads no meta for them.
    pub fn with_bound(mut self, collection: &str, bound: Bound) -> Self {
        self.bound = Some((collection.to_owned(), bound));
        self
    }

    /// Names a member of a kind named by its body without fetching the
    /// body, by its handle, when `bodies` is false: a dry run downloads
    /// nothing, and its replica is thrown away.
    pub fn with_bodies(mut self, bodies: bool) -> Self {
        self.bodies = bodies;
        self
    }

    /// [`wire_name`] against this side's namespace.
    fn wire_name<'n>(&self, collection: &'n str) -> &'n str {
        wire_name(&self.namespace, collection)
    }

    /// The refused creates, taken off so the driver can name them in reports.
    pub fn take_refused(&mut self) -> Vec<RefusedCreate> {
        mem::take(&mut self.refused)
    }

    /// The rejected writes, taken off so the driver can name them in reports.
    pub fn take_rejected(&mut self) -> Vec<RejectedPush> {
        mem::take(&mut self.rejected)
    }

    /// Records a write that did not land, answering the engine's rejection.
    ///
    /// A rejection is an outcome rather than an error: it makes io-pimdir keep
    /// the change and re-merge instead of clobbering the remote, where an error
    /// would abort the batch. Recording it keeps the run from claiming it.
    fn reject(
        &mut self,
        collection: &str,
        handle: PimdirHandle,
        action: &'static str,
        reason: impl fmt::Display,
    ) -> PimdirPushResult {
        let reason = reason.to_string();
        warn!("{action} {} in {collection} rejected: {reason}", handle.0);
        self.rejected.push(RejectedPush {
            collection: collection.to_string(),
            handle: handle.0.clone(),
            action,
            reason,
        });

        rejected_bare(handle)
    }
}

/// The name the backend knows a hub collection by.
///
/// This is where a `<namespace>/<name>` hub id becomes a name again, and every
/// wire call goes through it, the driver's fetch pool included. Only the
/// `<namespace>/` prefix goes; a leaked `/` is a mailbox name IMAP rejects.
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
/// rather than panicking keeps a construction path taking no `Result` honest.
pub(crate) fn resolve_kind(pool: &mut Pool) -> Kind {
    let media_type = pool.primary().media_type();
    Kind::from_media_type(media_type).unwrap_or_else(|| {
        warn!("unknown media type {media_type}, deriving link ids as mail");
        Kind::Mail
    })
}

/// How many bodies to request per batched fetch.
///
/// Larger cuts round trips but coarsens the retry unit and the command size.
pub(crate) const BATCH_SIZE: usize = 64;

/// Maps shared flags to their raw wire spelling.
///
/// A single side is internally consistent, and both sides run the same
/// normalization for the system flags that matter.
fn to_offline_flags<'f>(flags: impl IntoIterator<Item = &'f Flag>) -> PimdirFlags {
    flags.into_iter().map(|f| f.raw()).collect()
}

/// The item flags of a known set.
///
/// An unknown one (nothing has read the markers) yields none, which is what a
/// push of it would mean anyway: every backend here reports markers as it
/// enumerates, so only a store written by another owner can carry one.
fn to_item_flags(flags: &PimdirFlags) -> Vec<Flag> {
    let Some(flags) = flags.known() else {
        return Vec::new();
    };

    flags.iter().map(|s| Flag::from_raw(s.clone())).collect()
}

impl PimdirRemote for PimRemote<'_> {
    type Error = anyhow::Error;

    fn enumerate(
        &mut self,
        collection: &PimdirCollectionId,
        request: PimdirEnumerate,
    ) -> Result<PimdirEnumerated, Self::Error> {
        let hub = collection.as_str();
        let collection = self.wire_name(hub);

        let empty = Bound::default();
        let bound = match &self.bound {
            Some((of, bound)) if of == hub => bound,
            _ => &empty,
        };
        let held = Held {
            handles: &bound.handles,
            checkpoint: bound.checkpoint.as_deref(),
        };

        let listed = self
            .pool
            .primary()
            .enumerate(collection, &request, held)
            .with_context(|| format!("Enumerate {collection} error"))?;
        let page = match listed {
            Listed::Page(page) => page,
            Listed::CursorRejected => return Ok(PimdirEnumerated::CursorRejected),
        };
        self.pool.downloaded(page.bytes);

        // NOTE: a member is named by what the listing read, else by what the
        // store binds it under: a message never renames, and a resource
        // listed at the revision the store holds states what it stated.
        let mut items = Vec::with_capacity(page.items.len());
        let mut unnamed = Vec::new();
        for mut entry in page.items {
            let meta = match entry.meta.take() {
                Some(derivation) => Some(PimdirRemoteMeta::from(derivation)),
                // NOTE: a dry run names a changed resource by its binding too,
                // fetching no body: a pull without one, which the plan names.
                None => bound.metas.get(&entry.id).and_then(|known| {
                    let same = self.kind == Kind::Mail
                        || !self.bodies
                        || (entry.revision.is_some() && entry.revision == known.revision);
                    same.then(|| known.meta())
                }),
            };
            match meta {
                Some(meta) => items.push(PimdirRemoteItem {
                    handle: PimdirHandle::from(entry.id),
                    flags: to_offline_flags(&entry.flags),
                    revision: entry.revision,
                    meta,
                }),
                None if self.kind == Kind::Mail => {
                    warn!(
                        "{} in {collection} was listed without its meta, skipped",
                        entry.id
                    );
                }
                None => unnamed.push(entry),
            }
        }

        if !unnamed.is_empty() {
            items.extend(self.name_by_body(collection, unnamed)?);
        }

        keep_in_scope(&mut items, &request.scope);

        let vanished: Vec<PimdirHandle> =
            page.vanished.into_iter().map(PimdirHandle::from).collect();
        self.held.remember(collection, &items, &vanished);

        let snapshot = match page.complete {
            true => PimdirRemoteSnapshot {
                vanished,
                ..PimdirRemoteSnapshot::page(
                    items,
                    page.cursor.map(PimdirCursor),
                    page.checkpoint.map(PimdirCheckpoint),
                )
            },
            false => PimdirRemoteSnapshot {
                items,
                vanished,
                complete: false,
                last: true,
                cursor: None,
                checkpoint: page.checkpoint.map(PimdirCheckpoint),
            },
        };
        Ok(PimdirEnumerated::Page(snapshot))
    }

    fn scope_bound(&self) -> bool {
        self.scope_bound
    }

    fn fetch(
        &mut self,
        collection: &PimdirCollectionId,
        handles: Vec<PimdirHandle>,
        tier: PimdirTier,
    ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
        let collection = self.wire_name(collection.as_str());

        match tier {
            PimdirTier::Meta => self.fetch_meta(collection, handles),
            PimdirTier::Full => self.fetch_full(collection, handles),
        }
    }

    fn push(
        &mut self,
        collection: &PimdirCollectionId,
        changes: Vec<PimdirChange>,
    ) -> Result<Vec<PimdirPushResult>, Self::Error> {
        let collection = self.wire_name(collection.as_str()).to_string();
        let mut results = Vec::with_capacity(changes.len());

        for change in changes {
            let result = match change.kind {
                PimdirChangeKind::SetFlags { handle, flags } => {
                    let email_flags = to_item_flags(&flags);
                    let stored = self.pool.primary().store_flags(
                        &collection,
                        &[handle.as_str()],
                        &email_flags,
                        FlagOp::Set,
                    );

                    match stored {
                        Ok(()) => accepted(handle, None),
                        Err(err) => {
                            self.reject(&collection, handle, "set flags", format!("{err:#}"))
                        }
                    }
                }
                PimdirChangeKind::Remove {
                    handle,
                    to,
                    link_id: _,
                    if_match,
                } => match to {
                    Some(target) => {
                        let dest = wire_name(&self.namespace, target.as_str()).to_string();
                        let moved =
                            self.pool
                                .primary()
                                .move_items(&collection, &dest, &[handle.as_str()]);

                        match moved {
                            Ok(()) => accepted(handle, None),
                            Err(err) => {
                                self.reject(&collection, handle, "move", format!("{err:#}"))
                            }
                        }
                    }
                    None => {
                        let deleted = self.pool.primary().delete_item(
                            &collection,
                            handle.as_str(),
                            if_match.as_deref(),
                        );

                        match deleted {
                            Ok(()) => accepted(handle, None),
                            Err(err) => {
                                self.reject(&collection, handle, "delete", format!("{err:#}"))
                            }
                        }
                    }
                },
                PimdirChangeKind::Add {
                    handle,
                    link_id,
                    flags,
                    origin,
                    object,
                } => {
                    let link = link_id
                        .as_ref()
                        .map(|link| self.kind.split_link_id(link))
                        .unwrap_or_default();
                    match origin {
                        Some(origin) => {
                            self.copy(&collection, handle, &flags, &origin, object, link)
                        }
                        None => self.append(&collection, handle, &flags, object, link),
                    }
                }
                PimdirChangeKind::Update {
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

/// A [`PimdirRemote`] for applying one hydrated batch, serving its bodies.
///
/// The batch already streamed them in, so the `Full` upgrade does only index
/// writes. A miss falls back to a real fetch on the wrapped remote, correcting
/// a gap rather than losing it, and so do the rest.
pub struct CachedFetchRemote<R> {
    collection: String,
    cache: HashMap<String, PimdirFetchedItem>,
    fallback: R,
}

impl<R> CachedFetchRemote<R> {
    /// Serves one collection's fetched batch, falling back to `fallback`.
    pub fn new(collection: &str, items: Vec<PimdirFetchedItem>, fallback: R) -> Self {
        let cache = items
            .into_iter()
            .map(|item| (item.handle.0.clone(), item))
            .collect();
        Self {
            collection: collection.to_string(),
            cache,
            fallback,
        }
    }
}

impl<R: PimdirRemote<Error = anyhow::Error>> PimdirRemote for CachedFetchRemote<R> {
    type Error = anyhow::Error;

    fn enumerate(
        &mut self,
        collection: &PimdirCollectionId,
        request: PimdirEnumerate,
    ) -> Result<PimdirEnumerated, Self::Error> {
        self.fallback.enumerate(collection, request)
    }

    fn scope_bound(&self) -> bool {
        self.fallback.scope_bound()
    }

    fn fetch(
        &mut self,
        collection: &PimdirCollectionId,
        handles: Vec<PimdirHandle>,
        tier: PimdirTier,
    ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
        let mut items = Vec::with_capacity(handles.len());
        let mut misses = Vec::new();
        for handle in handles {
            let cached = (collection.as_str() == self.collection)
                .then(|| self.cache.remove(&handle.0))
                .flatten();
            match cached {
                Some(item) => items.push(item),
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
        collection: &PimdirCollectionId,
        changes: Vec<PimdirChange>,
    ) -> Result<Vec<PimdirPushResult>, Self::Error> {
        self.fallback.push(collection, changes)
    }
}

impl PimRemote<'_> {
    /// Names listed members of a kind named by its body (pimdir SYNC §4):
    /// their bodies fetched for the page, [`BATCH_SIZE`] a request across the
    /// pool, each carried by its member, the revision the body was read at
    /// winning over the listed one. A member whose body did not come is left
    /// out of the page, for the next run.
    fn name_by_body(
        &mut self,
        collection: &str,
        entries: Vec<EnumEntry>,
    ) -> Result<Vec<PimdirRemoteItem>> {
        if !self.bodies {
            return Ok(entries
                .into_iter()
                .map(|entry| PimdirRemoteItem {
                    meta: PimdirRemoteMeta {
                        link_id: PimdirLinkId(entry.id.clone()),
                        summary: None,
                        sort_key: PimdirSortKey::default(),
                        body: None,
                    },
                    handle: PimdirHandle::from(entry.id),
                    flags: to_offline_flags(&entry.flags),
                    revision: entry.revision,
                })
                .collect());
        }

        let handles = entries
            .iter()
            .map(|entry| PimdirHandle::from(entry.id.as_str()))
            .collect();
        let mut fetched: HashMap<String, PimdirFetchedItem> = self
            .fetch_full(collection, handles)?
            .into_iter()
            .map(|item| (item.handle.0.clone(), item))
            .collect();

        let mut items = Vec::with_capacity(entries.len());
        for entry in entries {
            let Some(item) = fetched.remove(&entry.id) else {
                warn!(
                    "{} in {collection} came without its body, skipped",
                    entry.id
                );
                continue;
            };
            items.push(PimdirRemoteItem {
                handle: item.handle,
                flags: to_offline_flags(&entry.flags),
                revision: item.revision.or(entry.revision),
                meta: PimdirRemoteMeta {
                    link_id: item.link_id,
                    summary: item.summary,
                    sort_key: item.sort_key,
                    body: item.body,
                },
            });
        }
        Ok(items)
    }

    /// Meta tier: a targeted summary fetch of just the requested handles.
    ///
    /// No bodies and no whole-collection sweep, so the cost scales with the
    /// change rather than with the collection.
    fn fetch_meta(
        &mut self,
        collection: &str,
        handles: Vec<PimdirHandle>,
    ) -> Result<Vec<PimdirFetchedItem>> {
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
            let Some(PimdirDerivation {
                link_id,
                summary,
                sort_key,
            }) = self.kind.parse_summary(env)
            else {
                continue;
            };
            items.push(PimdirFetchedItem {
                handle,
                link_id,
                summary,
                sort_key,
                body: None,
                revision: None,
            });
        }
        Ok(items)
    }

    /// Full tier: every body streamed straight into the blob store.
    ///
    /// Never held whole, and fetched in batches rather than one command per
    /// item, so N bodies cost about N/[`BATCH_SIZE`] round trips. Batches are
    /// work-stolen across a bounded pool of connections.
    fn fetch_full(
        &mut self,
        collection: &str,
        handles: Vec<PimdirHandle>,
    ) -> Result<Vec<PimdirFetchedItem>> {
        let items = self.fetch_bodies(collection, handles)?;
        self.pool.downloaded(body_bytes(&items));
        Ok(items)
    }

    /// The bodies of [`fetch_full`](Self::fetch_full), largest first when
    /// the sizes are known, across the pool.
    fn fetch_bodies(
        &mut self,
        collection: &str,
        mut handles: Vec<PimdirHandle>,
    ) -> Result<Vec<PimdirFetchedItem>> {
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
        let batches: Vec<Vec<PimdirHandle>> = handles
            .chunks(BATCH_SIZE)
            .map(<[PimdirHandle]>::to_vec)
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

    /// Fetches `batches` across up to `target` of the pool's connections.
    ///
    /// One shared queue, each worker draining it on its own connection.
    /// Work-stealing balances the load with no size probe, a worker with heavy
    /// batches naturally taking fewer.
    fn fetch_full_pooled(
        &mut self,
        collection: &str,
        batches: Vec<Vec<PimdirHandle>>,
        target: usize,
    ) -> Result<Vec<PimdirFetchedItem>> {
        let queue: SegQueue<Vec<PimdirHandle>> = SegQueue::new();
        for batch in batches {
            queue.push(batch);
        }
        let results: Mutex<Vec<PimdirFetchedItem>> = Mutex::new(Vec::new());
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

/// Streams one body into the blob store and reports the object by reference.
///
/// The link id and summary come from the streamed header prefix. Free of
/// `PimRemote` so a pool worker can call it on its own connection.
fn fetch_one_full(
    kind: Kind,
    client: &mut Client,
    collection: &str,
    handle: PimdirHandle,
    blob: &PimdirBlobs,
) -> Result<PimdirFetchedItem> {
    let writer = blob.writer().context("Open blob writer error")?;
    let mut sink = HydrateSink::new(writer, blob.hasher());
    let revision = client
        .get_item_stream(collection, handle.as_str(), &mut sink)
        .with_context(|| format!("Stream item {} in {collection} error", handle.as_str()))?;
    let (hash, size, header) = sink
        .finish()
        .with_context(|| format!("Commit body {} in {collection} error", handle.as_str()))?;

    // NOTE: no kind here has an empty body, and storing one names it by the
    // digest of nothing: every empty body a server hands back resolves to that
    // identity, each after the first filed as another copy of it.
    if size == 0 {
        bail!(
            "Server returned an empty body for {} in {collection}",
            handle.as_str(),
        );
    }

    let PimdirDerivation {
        link_id,
        summary,
        sort_key,
    } = kind.parse_body(&header, size as u64);
    Ok(PimdirFetchedItem {
        handle,
        link_id,
        summary,
        sort_key,
        body: Some(PimdirFetchedBody::Persisted { hash, size }),
        revision,
    })
}

/// Fetches a batch of bodies in one command, each streamed into its own blob.
///
/// The handle the server echoes keys each body back, so out-of-order responses
/// still route. A batch error, or a batch answering for fewer members than it
/// was asked, falls back to per-item fetches, idempotent by content address.
pub(crate) fn hydrate_batch(
    kind: Kind,
    client: &mut Client,
    collection: &str,
    handles: &[PimdirHandle],
    blob: &PimdirBlobs,
    on_body: Option<&(dyn Fn() + Sync)>,
) -> Result<Vec<PimdirFetchedItem>> {
    let ids: Vec<&str> = handles.iter().map(|h| h.as_str()).collect();
    let mut items: Vec<PimdirFetchedItem> = Vec::with_capacity(handles.len());

    let batched = client.fetch_bodies(
        collection,
        &ids,
        |_id| {
            blob.writer()
                .map(|writer| HydrateSink::new(writer, blob.hasher()))
        },
        |id, revision, sink: HydrateSink| {
            let (hash, size, header) = sink.finish().map_err(io::Error::other)?;
            let PimdirDerivation {
                link_id,
                summary,
                sort_key,
            } = kind.parse_body(&header, size as u64);
            items.push(PimdirFetchedItem {
                handle: PimdirHandle::from(id),
                link_id,
                summary,
                sort_key,
                body: Some(PimdirFetchedBody::Persisted { hash, size }),
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
            // NOTE: a short batch is not a batch that succeeded: the engine
            // would record nothing for the rest and ask again every run. A
            // CardDAV server was seen answering each card's ETag with a 404
            // body.
            let fetched: BTreeSet<String> = items
                .iter()
                .map(|item| item.handle.as_str().to_owned())
                .collect();
            let missing: Vec<PimdirHandle> = handles
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
    /// Appends a stored body as a genuine new member, checking the answer.
    ///
    /// A server may answer a create by updating the resource already holding
    /// the `UID` and handing its href back (RFC 6352 §6.3.2 forbids it). Two
    /// items on one handle read as one vanished, propagating a phantom delete.
    fn append(
        &mut self,
        collection: &str,
        handle: PimdirHandle,
        flags: &PimdirFlags,
        object: Option<PimdirHash>,
        link: LinkId<'_>,
    ) -> PimdirPushResult {
        let Some(hash) = object else {
            return self.reject(collection, handle, "append", "no body was stored for it");
        };

        let reader = match self.blob.reader(&hash) {
            Ok(Some(file)) => file,
            Ok(None) => {
                let reason = format!("its body {} is missing from the blob tree", hash.as_str());
                return self.reject(collection, handle, "append", reason);
            }
            Err(err) => {
                return self.reject(collection, handle, "append", format!("{err:#}"));
            }
        };
        let len = match reader.metadata() {
            Ok(meta) => meta.len() as usize,
            Err(err) => {
                return self.reject(collection, handle, "append", format!("{err:#}"));
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
                // NOTE: a duplicate `UID` gets its own report entry, naming the
                // identity and the remedy a generic rejection cannot state.
                if !is_duplicate_uid(&err) {
                    return self.reject(collection, handle, "append", format!("{err:#}"));
                }

                let uid = link.hint.unwrap_or(handle.as_str());
                warn!("append to {collection} refused: it already holds UID {uid}");
                self.refused.push(RefusedCreate {
                    collection: collection.to_string(),
                    uid: uid.to_string(),
                    handle: handle.0.clone(),
                });

                return rejected_bare(handle);
            }
        };

        self.assign(collection, handle, "append", written)
    }

    /// Creates a member by server-side copy from `origin` (pimdir SYNC §4)
    /// on a backend that copies, else by appending the stored body.
    ///
    /// The copy is accepted under the handle the server gives it, as an
    /// append is. A failed copy is rejected rather than uploaded: the create
    /// stays pending, and once the origin is gone the store offers it with
    /// none, as an append.
    fn copy(
        &mut self,
        collection: &str,
        handle: PimdirHandle,
        flags: &PimdirFlags,
        origin: &PimdirOrigin,
        object: Option<PimdirHash>,
        link: LinkId<'_>,
    ) -> PimdirPushResult {
        let from = wire_name(&self.namespace, origin.collection.as_str()).to_string();
        let copied = self
            .pool
            .primary()
            .copy_item(&from, collection, origin.handle.as_str());

        match copied {
            Ok(Some(written)) => self.assign(collection, handle, "copy", written),
            Ok(None) => self.append(collection, handle, flags, object, link),
            Err(err) => self.reject(collection, handle, "copy", format!("{err:#}")),
        }
    }

    /// Accepts a create under the handle the server assigned it, unless this
    /// side already holds that handle.
    fn assign(
        &mut self,
        collection: &str,
        handle: PimdirHandle,
        action: &'static str,
        written: WrittenItem,
    ) -> PimdirPushResult {
        let assigned = PimdirHandle::from(written.id);
        if !self.held.claim(collection, assigned.as_str()) {
            let reason = format!(
                "the server answered with {}, which it already holds",
                assigned.as_str(),
            );
            return self.reject(collection, handle, action, reason);
        }

        PimdirPushResult {
            handle,
            outcome: PimdirPushOutcome::Accepted,
            assigned: Some(assigned),
            revision: written.revision,
        }
    }

    /// Replaces an item's body in place, conditional on the synced revision.
    ///
    /// A refusal is [`PimdirPushOutcome::Rejected`] rather than an error: it
    /// is what makes io-pimdir re-merge and mark the placement conflicted
    /// instead of clobbering the remote body, an error aborting the batch.
    fn update(
        &mut self,
        collection: &str,
        handle: PimdirHandle,
        object: PimdirHash,
        if_match: Option<&str>,
    ) -> PimdirPushResult {
        let reader = match self.blob.reader(&object) {
            Ok(Some(file)) => file,
            Ok(None) => {
                let reason = format!("its body {} is missing from the blob tree", object.as_str());
                return self.reject(collection, handle, "update", reason);
            }
            Err(err) => {
                return self.reject(collection, handle, "update", format!("{err:#}"));
            }
        };
        let len = match reader.metadata() {
            Ok(meta) => meta.len() as usize,
            Err(err) => {
                return self.reject(collection, handle, "update", format!("{err:#}"));
            }
        };

        let updated = self.pool.primary().update_item_stream(
            collection,
            handle.as_str(),
            reader,
            len,
            if_match,
        );

        match updated {
            Ok(revision) => PimdirPushResult {
                handle,
                outcome: PimdirPushOutcome::Accepted,
                assigned: None,
                revision,
            },
            Err(err) => self.reject(collection, handle, "update", format!("{err:#}")),
        }
    }
}

/// The octets of the bodies a fetch brought in.
pub(crate) fn body_bytes(items: &[PimdirFetchedItem]) -> u64 {
    items
        .iter()
        .map(|item| match &item.body {
            Some(PimdirFetchedBody::Persisted { size, .. }) => *size as u64,
            Some(PimdirFetchedBody::Inline { bytes, .. }) => bytes.len() as u64,
            None => 0,
        })
        .sum()
}

fn accepted(handle: PimdirHandle, assigned: Option<PimdirHandle>) -> PimdirPushResult {
    PimdirPushResult {
        handle,
        outcome: PimdirPushOutcome::Accepted,
        assigned,
        revision: None,
    }
}

fn rejected_bare(handle: PimdirHandle) -> PimdirPushResult {
    PimdirPushResult {
        handle,
        outcome: PimdirPushOutcome::Rejected,
        assigned: None,
        revision: None,
    }
}

/// Cap on captured header bytes, bounding memory if no boundary is found.
const HEADER_CAP: usize = 256 * 1024;

/// A [`Write`] sink for a streaming `Full` fetch.
///
/// It tees each chunk into the blob store, folds it into the content hash, and
/// captures the header prefix so the link id and summary parse without a second
/// pass. The whole body never sits in memory.
struct HydrateSink {
    writer: PimdirBlobWriter,
    hasher: PimdirHasher,
    header: Vec<u8>,
    header_done: bool,
}

impl HydrateSink {
    /// Builds the sink over a blob writer and the store's own hasher.
    ///
    /// A body is then named by the algorithm the store records (pimdir SPEC §5)
    /// and dedups against what any other consumer of that store wrote.
    fn new(writer: PimdirBlobWriter, hasher: PimdirHasher) -> Self {
        Self {
            writer,
            hasher,
            header: Vec::new(),
            header_done: false,
        }
    }

    /// Commits the blob under its hash, returning `(hash, size, header bytes)`.
    fn finish(self) -> Result<(PimdirHash, usize, Vec<u8>)> {
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

/// The byte offset just past the header and body boundary, or `None`.
fn header_boundary(buf: &[u8]) -> Option<usize> {
    buf.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| buf.windows(2).position(|w| w == b"\n\n").map(|i| i + 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every wire call goes through this seam.
    ///
    /// A hub id that reaches a server is rejected outright on IMAP, whose
    /// mailbox names cannot hold the `/` a namespace prefix ends on.
    #[test]
    fn a_hub_id_becomes_the_name_its_server_knows() {
        assert_eq!(
            wire_name("imap", "imap/Archives.Charlie"),
            "Archives.Charlie"
        );
        assert_eq!(wire_name("mail", "mail/INBOX"), "INBOX");
        assert_eq!(wire_name("mail", "mail/Archive/2026"), "Archive/2026");
        assert_eq!(wire_name("mail", "mailbox/INBOX"), "mailbox/INBOX");
        assert_eq!(wire_name("cards", "mail/INBOX"), "mail/INBOX");
    }

    /// One enumerated member, the shape [`HeldHandles::remember`] folds in.
    fn member(handle: &str) -> PimdirRemoteItem {
        PimdirRemoteItem {
            handle: PimdirHandle(handle.into()),
            flags: PimdirFlags::default(),
            revision: None,
            meta: PimdirRemoteMeta {
                link_id: PimdirLinkId(handle.into()),
                summary: None,
                sort_key: PimdirSortKey::default(),
                body: None,
            },
        }
    }

    /// A server updating the `UID`'s resource hands back an href this holds.
    ///
    /// Two items on one handle make the next enumeration read one as vanished,
    /// propagating a delete nobody asked for, so the create is refused.
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

    /// A member the server reported gone is free again.
    ///
    /// Its href may be reused, and refusing a create onto a resource nobody
    /// holds would keep an item unwritable for the rest of the run.
    #[test]
    fn a_vanished_handle_stops_being_held() {
        let mut held = HeldHandles::default();
        held.remember("agenda", &[member("event-1.ics")], &[]);
        held.remember("agenda", &[], &[PimdirHandle("event-1.ics".into())]);

        assert!(held.claim("agenda", "event-1.ics"));
    }
}

#[cfg(test)]
mod scope_tests {
    use io_pimdir::summary::mail;

    use super::*;

    /// A listed message named by a header block carrying `date`.
    fn dated(handle: &str, date: Option<&str>) -> PimdirRemoteItem {
        let mut header = format!("Message-ID: <{handle}@example.org>\r\n");
        if let Some(date) = date {
            header.push_str(&format!("Date: {date}\r\n"));
        }
        header.push_str("\r\n");

        PimdirRemoteItem {
            handle: PimdirHandle(handle.into()),
            flags: PimdirFlags::default(),
            revision: None,
            meta: PimdirRemoteMeta::from(mail::derive_meta(header.as_bytes(), None, None)),
        }
    }

    fn handles(items: &[PimdirRemoteItem]) -> Vec<&str> {
        items.iter().map(|item| item.handle.as_str()).collect()
    }

    /// The `Date` decides, whatever narrowed the listing: a message received
    /// today but dated years ago is out, one dated in the future is in, and
    /// one with no usable date is in every scope.
    #[test]
    fn a_page_keeps_what_its_date_puts_in_scope() {
        let mut items = vec![
            dated("old", Some("Wed, 01 Jan 2020 10:00:00 +0000")),
            dated("edge", Some("Thu, 01 Oct 2026 02:00:00 +0200")),
            dated("recent", Some("Mon, 05 Oct 2026 10:00:00 +0000")),
            dated("future", Some("Tue, 01 Jan 2030 10:00:00 +0000")),
            dated("undated", None),
            dated("garbled", Some("sometime last week")),
        ];

        keep_in_scope(&mut items, &PimdirScope::since("2026-10-01T00:00:00Z"));

        assert_eq!(
            handles(&items),
            ["edge", "recent", "future", "undated", "garbled"],
            "02:00 at +02:00 is midnight UTC, the floor itself",
        );
    }

    /// A band lists below the coverage only: its ceiling is exclusive.
    #[test]
    fn a_band_keeps_what_lies_below_its_ceiling() {
        let mut items = vec![
            dated("older", Some("Wed, 01 Jan 2025 10:00:00 +0000")),
            dated("covered", Some("Thu, 01 Oct 2026 00:00:00 +0000")),
        ];
        let band = PimdirScope {
            since: Some(String::from("2024-01-01T00:00:00Z")),
            until: Some(String::from("2026-10-01T00:00:00Z")),
        };

        keep_in_scope(&mut items, &band);

        assert_eq!(handles(&items), ["older"]);
    }

    /// A member the store binds is named by its binding, whose sort key is
    /// its date: the scope reads it there.
    #[test]
    fn a_bound_member_is_in_scope_by_its_stored_date() {
        let bound = BoundMeta {
            revision: None,
            link_id: PimdirLinkId("old@example.org".into()),
            sort_key: PimdirSortKey("2020-01-01T10:00:00Z".into()),
        };
        let mut items = vec![PimdirRemoteItem {
            handle: PimdirHandle("7".into()),
            flags: PimdirFlags::default(),
            revision: None,
            meta: bound.meta(),
        }];

        keep_in_scope(&mut items, &PimdirScope::unbounded());
        assert_eq!(items.len(), 1, "an unbounded scope keeps everything");

        keep_in_scope(&mut items, &PimdirScope::since("2026-10-01T00:00:00Z"));
        assert!(items.is_empty());
    }
}
