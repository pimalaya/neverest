//! Per-account/per-collection orchestration over the io-replica engine.
//!
//! Each collection's two sides are the two *sources* of one shared collection in a
//! pimdir store: one [`PimdirStore`] handle per side (`"left"` / `"right"`) over
//! the same files, the collection name as the bare collection id. The driver only
//! runs the engine's per-side `sync` (and a `Meta` `upgrade` to resolve link
//! ids, plus a `Full` upgrade to hydrate a body about to be copied): cross-side
//! propagation of items, flags and deletions falls out of the shared hub's
//! project/absorb, so there is no hand-rolled cross-merge. Syncing the sides in
//! turn until quiescent converges them (a change one side folds into the hub
//! reads as dirty for the other, which its next sync pushes). The topology
//! mismatch this resolves is in the crate header.
//!
//! First-cut gaps: collection deletion is not propagated (only creation), and the
//! itemized report lists cross-side copies/flags/deletes (the per-side server
//! reconcile is internal). A `Full` fetch opens a small connection pool to stream
//! several bodies at once, largest-first; the rest of a side runs on one
//! connection.

use std::{
    cmp::Reverse,
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

use anyhow::{Context, Result, anyhow, bail};
use crossbeam_queue::SegQueue;
use io_pimdir::{PimdirBlobs, PimdirError, PimdirStore};
use io_replica::{
    client::{ReplicaRemote, ReplicaStorage},
    collection::ReplicaCollectionId,
    coroutine::{ReplicaArg, ReplicaCoroutine, ReplicaCoroutineState, ReplicaYield},
    placement::{
        ReplicaFlags, ReplicaHandle, ReplicaLevel, ReplicaMeta, ReplicaPlacement, ReplicaStatus,
    },
    rekey::{ReplicaRekey, ReplicaRekeyReport},
    remote::{ReplicaFetchedItem, ReplicaTier},
    sync::{ReplicaEvent, ReplicaPushRights, ReplicaSync, ReplicaSyncOptions, ReplicaSyncReport},
    upgrade::ReplicaUpgrade,
};
use log::{info, warn};
use pimalaya_cli::spinner::Spinner;

use crate::{
    client::{Client, Pool},
    config::{
        AccountConfig, CollectionFilter, Hydration, Retention, SideConfig, SideName,
        SidePermissions,
    },
    item::flag::Flag,
    kind::Kind,
    offline::{
        drive, pipe,
        remote::{BATCH_SIZE, CachedFetchRemote, FetchKey, PimRemote, hydrate_batch, resolve_kind},
        source_id,
        storage::{hydration_targets, load_side, projection_view},
        submit,
    },
    side::Side,
    sync::{
        hunk::{CollectionHunk, ItemHunk},
        report::{
            DrainedQueue, ItemConflict, ParkedQueueAction, PatchEntry, PurgedItems, SyncReport,
        },
    },
};
#[cfg(any(feature = "smtp", feature = "msgraph"))]
use crate::{config::SmtpConfig, sync::report::SubmitEntry};

/// How many extra sync passes to run after the first, propagating and settling
/// cross-side changes. Two or three passes converge a collection in practice; the
/// cap only guards against a pathological loop.
const MAX_EXTRA_PASSES: usize = 4;

/// Resolves `<state_dir>/neverest/<account>/`, the default replica root.
pub fn replica_dir(account: &str) -> Result<PathBuf> {
    let base = dirs::state_dir().context("Cannot resolve XDG state directory")?;
    Ok(base.join("neverest").join(account))
}

/// The account's pimdir store directory: the configured `store.root` override,
/// else the default per-account state directory.
pub fn store_dir(account: &str, config: &AccountConfig) -> Result<PathBuf> {
    match &config.store.root {
        Some(root) => Ok(root.clone()),
        None => replica_dir(account),
    }
}

/// Opens one source handle over the account's store, grouping every
/// collection it writes under `account` (pimdir SPEC §9.2) so a store two
/// hand-written accounts share tells whose collection is whose without a
/// reader guessing from the naming.
///
/// A store an earlier draft of the format wrote cannot be migrated in place
/// (the spec is a draft, and the reference schema keeps folding into version
/// 1), so the refusal is answered with the one command that fixes it: the
/// store is a derived cache, and dropping it costs a resync.
fn open_store(dir: &Path, source: &str, account: &str) -> Result<PimdirStore> {
    match PimdirStore::open(dir, source) {
        Ok(store) => Ok(store.for_account(account)),
        Err(err @ (PimdirError::Version { .. } | PimdirError::Unreconcilable { .. })) => Err(
            anyhow::Error::new(err).context(format!(
                "The replica store predates this neverest; drop it with `neverest sync --reset -a {account}` and let it resync"
            )),
        ),
        Err(err) => Err(anyhow::Error::new(err).context(format!("Open {source} store"))),
    }
}

/// The one kind a two-side account syncs, refusing a pair whose backends
/// disagree.
///
/// The kind is never configured — it falls out of each side's backend — so the
/// configuration schema cannot express "these two sides are compatible" and
/// this check is the enforcement point. It runs after the sides open (the
/// earliest moment their media types are known) and before the first store
/// write, so a mismatched account fails cleanly with nothing half-written.
fn check_kinds(left: &mut SideCtx, right: &mut SideCtx) -> Result<Kind> {
    let left = (left.side, left.pool.primary().media_type());
    let right = (right.side, right.pool.primary().media_type());
    pair_kind(left, right)
}

/// The pure half of [`check_kinds`]: the two sides' media types in, the one
/// kind they agree on out.
fn pair_kind(left: (Side, &str), right: (Side, &str)) -> Result<Kind> {
    let resolve = |(side, raw): (Side, &str)| -> Result<Kind> {
        Kind::from_media_type(raw)
            .with_context(|| format!("This build cannot sync items of type `{raw}` ({side} side)"))
    };

    let left_kind = resolve(left)?;
    let right_kind = resolve(right)?;

    if left_kind != right_kind {
        bail!(
            "The two sides sync different kinds and cannot be reconciled: \
             {} is `{}` and {} is `{}`. \
             Pair each kind with its own account (they may share a `store.root`).",
            left.0,
            left_kind.media_type(),
            right.0,
            right_kind.media_type(),
        );
    }

    Ok(left_kind)
}

/// Live per-collection progress: the collection spinner and its base label, so an inner
/// phase (body hydration, relay) can append a percentage to the `[2/7] Syncing
/// INBOX` line (`… Syncing INBOX 66%`).
struct MailboxProgress<'a> {
    spinner: &'a Spinner,
    label: &'a str,
}

impl MailboxProgress<'_> {
    /// Updates the spinner to `<label> <percent>%` of `done`/`total`.
    fn tick(&self, done: usize, total: usize) {
        let percent = (done * 100).checked_div(total).unwrap_or(100);
        self.spinner
            .set_message(format!("{} ({percent}%)", self.label));
    }
}

/// One connected side plus its permissions and whether it may push.
struct SideCtx {
    side: Side,
    pool: Pool,
    perms: SidePermissions,
}

impl SideCtx {
    /// A side pushes unless it forbids every item and flag mutation (a fully
    /// read-only source, e.g. the remote leg of a backup).
    fn writable(&self) -> bool {
        let rights = self.push_rights();
        rights.flags || rights.content || rights.add || rights.remove
    }

    /// The side's configured permissions as io-replica's per-kind push rights.
    ///
    /// The two vocabularies line up one to one, so this is a mapping rather
    /// than a policy. A forbidden kind is kept pending by the engine (never
    /// pushed, and a forbidden delete is not applied to the replica either)
    /// while the other kinds still propagate — which is what makes a side
    /// read-only for *some* operations, rather than all or nothing.
    fn push_rights(&self) -> ReplicaPushRights {
        ReplicaPushRights {
            flags: self.perms.flag.update,
            content: self.perms.item.update,
            add: self.perms.item.create,
            remove: self.perms.item.delete,
        }
    }
}

/// Runs the whole account sync and returns the report. Dispatches on the number
/// of configured sides: one is a **local sync** (that remote against the retained
/// pimdir store the app reads), two is a **remote-to-remote** sync through the
/// store, cross-side propagation falling out of the shared hub.
pub fn run(
    account_name: impl Into<String>,
    account_config: &AccountConfig,
    mailbox_filter: Option<CollectionFilter>,
    dry_run: bool,
    connections: usize,
    no_purge: bool,
) -> Result<SyncReport> {
    let account_name = account_name.into();
    match (account_config.left.clone(), account_config.right.clone()) {
        (None, None) => {
            bail!("Account `{account_name}` has no side configured (set `left` and/or `right`)")
        }
        (Some(side), None) => run_single(
            account_name,
            account_config,
            SideName::Left,
            side,
            mailbox_filter,
            dry_run,
            connections,
            no_purge,
        ),
        (None, Some(side)) => run_single(
            account_name,
            account_config,
            SideName::Right,
            side,
            mailbox_filter,
            dry_run,
            connections,
            no_purge,
        ),
        (Some(left), Some(right)) => run_dual(
            account_name,
            account_config,
            left,
            right,
            mailbox_filter,
            dry_run,
            connections,
            no_purge,
        ),
    }
}

/// The remote-to-remote (two-source) sync.
#[allow(clippy::too_many_arguments)]
fn run_dual(
    account_name: String,
    account_config: &AccountConfig,
    left_config: SideConfig,
    right_config: SideConfig,
    mailbox_filter: Option<CollectionFilter>,
    dry_run: bool,
    connections: usize,
    no_purge: bool,
) -> Result<SyncReport> {
    let mailbox_filter = mailbox_filter.unwrap_or_else(|| account_config.collection.filter.clone());

    let mut report = SyncReport {
        account: account_name.clone(),
        dry_run,
        ..Default::default()
    };

    // NOTE: the real replica; in dry-run everything runs against a throwaway
    // copy of the store, so no checkpoint advances and no server is written.
    let real_dir = store_dir(&account_name, account_config)?;
    let work_dir = if dry_run {
        dry_run_replica(&real_dir)?
    } else {
        real_dir.clone()
    };
    fs::create_dir_all(&work_dir)
        .with_context(|| format!("Create replica dir `{}` error", work_dir.display()))?;

    // NOTE: two source handles over one pimdir store — `"left"` and `"right"`
    // bindings of each shared item. The blob reader is independent of either
    // connection, so a remote can read a stored body back while a store is
    // borrowed to service a sync. It comes from a store handle rather than
    // the directory, so bodies are named by the hash the store records
    // (pimdir SPEC §5) instead of one this side picks.
    let mut left_store = open_store(&work_dir, "left", &account_name)?;
    let mut right_store = open_store(&work_dir, "right", &account_name)?;
    let blobs = left_store.blobs();

    // NOTE: neverest is the store's sole owner — apply what the frontends
    // queued before any network work, so this run's sync pushes the resulting
    // dirty state.
    drain_queues(&mut left_store, &mut report);

    // NOTE: `hydration = "full"` mirrors every body, which needs retention.
    let hydrate_full = account_config.store.hydration == Some(Hydration::Full);

    // NOTE: retention — a two-side sync relays (streams a body server-to-server,
    // the store keeping only the spine) by default when both sides are IMAP
    // (relay is IMAP-first); any other pairing, or `store.retention = "retain"`,
    // retains. An explicit `relay` on a non-IMAP pairing falls back to retain,
    // as does one combined with `hydration = "full"`.
    let relay_capable = left_config.is_imap() && right_config.is_imap();
    let relay = match account_config.store.retention {
        Some(Retention::Retain) => false,
        Some(Retention::Relay) => {
            if !relay_capable {
                warn!("relay requested but a side is not IMAP; retaining instead");
            }
            if hydrate_full {
                warn!("relay requested but hydration is full; retaining instead");
            }
            relay_capable && !hydrate_full
        }
        None => relay_capable && !hydrate_full,
    };

    // NOTE: HTTP backends (Graph) keep one connection: extra ones only
    // pay extra token acquisitions, the API is request/response anyway.
    let left_imap = left_config.is_imap();
    let right_imap = right_config.is_imap();
    let left_budget = if left_imap { connections } else { 1 };
    let right_budget = if right_imap { connections } else { 1 };

    let s = Spinner::start("Opening sides…");
    let mut left = SideCtx {
        side: Side::Left,
        perms: left_config.permissions(),

        pool: Pool::open(left_config, left_budget).context("Open left side")?,
    };
    let mut right = SideCtx {
        side: Side::Right,
        perms: right_config.permissions(),
        pool: Pool::open(right_config, right_budget).context("Open right side")?,
    };
    s.success("Opened sides");

    // The item kind recorded on every collection this account touches. It is
    // never configured: it falls out of each side's backend
    // (`Client::media_type`), so the two sides must agree before anything is
    // written — a mailbox cannot reconcile against an address book. The
    // configuration schema cannot express the constraint (a side is just a
    // backend), so this is where it is enforced, at runtime, right after the
    // sides open and before the first store write.
    let kind = check_kinds(&mut left, &mut right)?;
    let media_type = kind.media_type();

    // NOTE: the queue's capability-bound half: the pre-network drain applied
    // every store mutation and left the intents only this build can perform.
    if !dry_run {
        drain_submits(
            account_config,
            &mut [&mut left, &mut right],
            &mut left_store,
            &blobs,
            &mut report,
        );
    }

    let s = Spinner::start("Listing collections…");
    let left_mailboxes = list_collections(left.pool.primary())?;
    let right_mailboxes = list_collections(right.pool.primary())?;
    s.success(format!(
        "Listed collections ({} left, {} right)",
        left_mailboxes.len(),
        right_mailboxes.len()
    ));

    let left_filtered = filter_mailboxes(&left_mailboxes, &mailbox_filter);
    let right_filtered = filter_mailboxes(&right_mailboxes, &mailbox_filter);

    let mailbox_hunks = diff_mailboxes(&left_filtered, &right_filtered, &left, &right);
    for hunk in mailbox_hunks {
        let error = if dry_run {
            None
        } else {
            apply_mailbox_hunk(&hunk, &mut left, &mut right).err()
        };
        report.collection.patch.push(PatchEntry::new(hunk, error));
    }

    // NOTE: item sync runs over collections present (post-create) on both
    // sides.
    let mut present_left = left_filtered.clone();
    let mut present_right = right_filtered.clone();
    for entry in &report.collection.patch {
        if entry.error.is_some() {
            continue;
        }
        if let CollectionHunk::Create { side, collection } = &entry.hunk {
            match side {
                Side::Left => present_left.insert(collection.clone()),
                Side::Right => present_right.insert(collection.clone()),
            };
        }
    }
    let common: BTreeSet<String> = present_left.intersection(&present_right).cloned().collect();

    let total = common.len();
    for (index, collection) in common.iter().enumerate() {
        let label = format!("[{}/{total}] Syncing {collection}", index + 1);
        let s = Spinner::start(label.clone());
        let progress = MailboxProgress {
            spinner: &s,
            label: &label,
        };

        if let Err(err) = sync_mailbox(
            collection,
            media_type,
            &mut left,
            &mut right,
            &mut left_store,
            &mut right_store,
            &blobs,
            dry_run,
            relay,
            &progress,
            &mut report,
        ) {
            warn!("`{collection}` sync error: {err:#}");
            s.success(format!("{collection}: error ({err:#})"));
            continue;
        }

        // NOTE: `hydration = "full"` — mirror every body, not only the
        // crossing ones; the shared object store dedups across sides.
        if hydrate_full
            && !dry_run
            && let Err(err) = hydrate_full_mailbox(
                collection,
                &mut left,
                &mut right,
                &mut left_store,
                &mut right_store,
                &blobs,
                &progress,
            )
        {
            warn!("`{collection}` full hydration error: {err:#}");
        }

        s.success(format!("{collection}: done"));
    }

    // NOTE: the retention sweep runs after the sync, so an item this run
    // retired starts its delay now rather than being reclaimed by the very
    // run that retired it.
    if !dry_run && !no_purge {
        sweep_retained(account_config, &mut left_store, &mut report);
    }

    // NOTE: pimdir commits each write batch, so there is nothing to flush; a
    // dry run just discards its throwaway store copy.
    if dry_run {
        let _ = fs::remove_dir_all(&work_dir);
    }

    crate::offline::prof::report();
    Ok(report)
}

/// A collection's spine result: its name and the bodies to hydrate (handle + size),
/// carried from Phase 1 (spine) into Phase 2 (hydrate) and Phase 3 (apply).
type MailboxPlan = (String, Vec<(ReplicaHandle, u64)>);

/// The local (one-source) sync, run as three account-wide phases so the
/// connection pool stays saturated end to end (never idling at collection
/// boundaries): **Phase 1** spines every collection in parallel (enumerate + meta +
/// push) over its own connection and store handle, collecting the bodies to
/// hydrate; **Phase 2** streams every body across all collections through one global
/// work-stealing pool into the blob store; **Phase 3** applies the fetched bodies
/// to the index per collection from cache, no network. The store is the single local
/// copy the app reads; there is no cross-side hydration.
#[allow(clippy::too_many_arguments)]
fn run_single(
    account_name: String,
    account_config: &AccountConfig,
    side_name: SideName,
    side_config: SideConfig,
    mailbox_filter: Option<CollectionFilter>,
    dry_run: bool,
    connections: usize,
    no_purge: bool,
) -> Result<SyncReport> {
    let mailbox_filter = mailbox_filter.unwrap_or_else(|| account_config.collection.filter.clone());

    let mut report = SyncReport {
        account: account_name.clone(),
        dry_run,
        ..Default::default()
    };

    let real_dir = store_dir(&account_name, account_config)?;
    let work_dir = if dry_run {
        dry_run_replica(&real_dir)?
    } else {
        real_dir.clone()
    };
    fs::create_dir_all(&work_dir)
        .with_context(|| format!("Create replica dir `{}` error", work_dir.display()))?;

    // NOTE: the store is opened as the one side's source, so an app writing as
    // that same source (himalaya auto-detects it) stages edits this sync pushes.
    // We open one store handle *per connection*: Phase 1 fans the spine across
    // them, their writes serialising on the store's single-writer lock while the
    // network overlaps.
    let side = match side_name {
        SideName::Left => Side::Left,
        SideName::Right => Side::Right,
    };
    let source = side_name.to_string();
    // NOTE: HTTP backends (Graph) keep one connection: extra ones only pay
    // extra token acquisitions, the API is request/response anyway.
    let workers = if side_config.is_imap() {
        connections.max(1)
    } else {
        1
    };
    let s = Spinner::start("Opening connections…");
    let mut ctxs = open_side_contexts(side, &side_config, workers)?;
    let mut stores: Vec<PimdirStore> = (0..workers)
        .map(|_| open_store(&work_dir, &source, &account_name))
        .collect::<Result<_>>()?;
    // NOTE: from a store handle, so a body is named by the hash the store
    // records (pimdir SPEC §5) rather than one this side picks.
    let blobs = stores[0].blobs();
    s.success(format!("Opened {} connection(s)", ctxs.len()));

    // NOTE: neverest is the store's sole owner — apply what the frontends
    // queued before any network work, so this run's sync pushes the resulting
    // dirty state.
    drain_queues(&mut stores[0], &mut report);

    // One side, so there is no cross-side pairing to check — only that this
    // build knows the backend's kind at all.
    let raw = ctxs[0].pool.primary().media_type();
    let kind = Kind::from_media_type(raw)
        .with_context(|| format!("This build cannot sync items of type `{raw}`"))?;
    let media_type = kind.media_type();

    // NOTE: the queue's capability-bound half: the pre-network drain applied
    // every store mutation and left the intents only this build can perform.
    if !dry_run {
        let (first, _) = ctxs.split_first_mut().expect("at least one connection");
        drain_submits(
            account_config,
            &mut [first],
            &mut stores[0],
            &blobs,
            &mut report,
        );
    }

    let s = Spinner::start("Listing collections…");
    let collections = list_collections(ctxs[0].pool.primary())?;
    s.success(format!("Listed collections ({})", collections.len()));
    let filtered: Vec<String> = filter_mailboxes(&collections, &mailbox_filter)
        .into_iter()
        .collect();

    // Pre-create every collection serially, so no Phase-1 worker races on lazy
    // collection creation (and each carries its declared media type).
    for collection in &filtered {
        stores[0]
            .ensure_collection(collection, media_type)
            .with_context(|| format!("Declare kind for `{collection}`"))?;
    }

    // === Phase 1: spine (parallel over collections) ===
    let plans = phase1_spine(
        &filtered,
        &mut ctxs,
        &mut stores,
        &blobs,
        dry_run,
        &mut report,
    )?;

    if dry_run {
        let _ = fs::remove_dir_all(&work_dir);
        crate::offline::prof::report();
        return Ok(report);
    }

    // === Phase 2: hydrate (one global work-stealing pool over every body) ===
    let cache = phase2_hydrate(&plans, &mut ctxs, &blobs)?;

    // === Phase 3: apply the fetched bodies to the index, per collection, no network ===
    phase3_apply(&plans, &mut ctxs[0], &mut stores[0], &blobs, &cache)?;

    // NOTE: the retention sweep runs after the sync, so an item this run
    // retired starts its delay now rather than being reclaimed by the very
    // run that retired it.
    if !no_purge {
        sweep_retained(account_config, &mut stores[0], &mut report);
    }

    crate::offline::prof::report();
    Ok(report)
}

/// Opens `count` independent single-connection [`SideCtx`]s in parallel, so the
/// connection handshakes overlap instead of paying them one after another.
fn open_side_contexts(side: Side, config: &SideConfig, count: usize) -> Result<Vec<SideCtx>> {
    let perms = config.permissions();
    let opened: Vec<Result<Pool>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..count)
            .map(|_| {
                let cfg = config.clone();
                scope.spawn(move || Pool::open(cfg, 1))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("connection open thread"))
            .collect()
    });
    let mut ctxs = Vec::with_capacity(count);
    for pool in opened {
        ctxs.push(SideCtx {
            side,
            perms,
            pool: pool.context("Open connection")?,
        });
    }
    Ok(ctxs)
}

/// Phase 1 — spine each collection in parallel: a work-stealing pool of workers, each
/// on its own connection and store handle, pulls the collection queue and reconciles
/// (pull + meta + itemize + push), collecting the bodies to hydrate. Reads run
/// concurrently (WAL); writes serialise on the store's single-writer lock. Report
/// patches are merged after the barrier. A collection that errors is logged and
/// skipped (its bodies simply don't enter the plan).
fn phase1_spine(
    filtered: &[String],
    ctxs: &mut [SideCtx],
    stores: &mut [PimdirStore],
    blobs: &PimdirBlobs,
    dry_run: bool,
    report: &mut SyncReport,
) -> Result<Vec<MailboxPlan>> {
    let total = filtered.len();
    let queue: SegQueue<String> = SegQueue::new();
    for collection in filtered {
        queue.push(collection.clone());
    }
    let plans: Mutex<Vec<MailboxPlan>> = Mutex::new(Vec::new());
    let merged: Mutex<SyncReport> = Mutex::new(SyncReport::default());
    let scanned = AtomicUsize::new(0);
    let s = Spinner::start(format!("Scanning collections (0/{total})"));

    let queue_ref = &queue;
    let plans_ref = &plans;
    let merged_ref = &merged;
    let scanned_ref = &scanned;
    let s_ref = &s;

    thread::scope(|scope| {
        for (ctx, store) in ctxs.iter_mut().zip(stores.iter_mut()) {
            scope.spawn(move || {
                while let Some(collection) = queue_ref.pop() {
                    match mailbox_spine(&collection, ctx, store, blobs, dry_run) {
                        Ok((targets, rep)) => {
                            plans_ref.lock().unwrap().push((collection, targets));
                            merged_ref.lock().unwrap().item.patch.extend(rep.item.patch);
                        }
                        Err(err) => warn!("`{collection}` scan error: {err:#}"),
                    }
                    let n = scanned_ref.fetch_add(1, Ordering::Relaxed) + 1;
                    s_ref.set_message(format!("Scanning collections ({n}/{total})"));
                }
            });
        }
    });

    let plans = plans.into_inner().unwrap();
    report
        .item
        .patch
        .extend(merged.into_inner().unwrap().item.patch);
    s.success(format!("Scanned {} collection(s)", total));
    Ok(plans)
}

/// Reconciles one collection's spine (no hydration): pull the remote into the hub,
/// itemize the pending local→remote edits and the pull plan, then push and settle,
/// and return the not-yet-`Full` bodies to hydrate (handle + size from the local
/// envelope meta, for a largest-first download) plus the report patches. A dry run
/// stops after itemizing (targets empty).
fn mailbox_spine(
    collection: &str,
    ctx: &mut SideCtx,
    store: &mut PimdirStore,
    blobs: &PimdirBlobs,
    dry_run: bool,
) -> Result<(Vec<(ReplicaHandle, u64)>, SyncReport)> {
    let mut report = SyncReport::default();

    // NOTE: snapshot flags before the pull so remote flag changes (which the pull
    // applies silently, leaving the item Clean) can be itemized from the sync's
    // per-item events rather than the post-pull projection.
    let before = flag_snapshot(store, collection, ctx.side)?;

    // NOTE: pull only first, so the projection still carries any edit the app
    // staged locally (kept dirty because we did not push it yet).
    let pull = sync_side_rebuilding(collection, ctx, store, blobs, false)?;
    itemize_pulled(
        &pull.events,
        &before,
        store,
        collection,
        ctx.side,
        &mut report,
    )?;
    upgrade_probed(collection, ctx, store, blobs)?;
    itemize_single(collection, store, ctx, &mut report)?;
    itemize_fetches(collection, store, ctx.side, &mut report)?;
    if dry_run {
        return Ok((Vec::new(), report));
    }

    // NOTE: push the staged local edits to the remote and settle.
    for _ in 0..=MAX_EXTRA_PASSES {
        let pass = sync_side_rebuilding(collection, ctx, store, blobs, ctx.writable())?;
        upgrade_probed(collection, ctx, store, blobs)?;
        if !moved(&pass) {
            break;
        }
    }

    // NOTE: the bodies to hydrate — every item with no stored body — with its
    // size from the already-local envelope meta, so Phase 2 can order
    // largest-first. The body is what is asked for, so the body is what is
    // tested: a remote content change drops the stale one while the hub keeps
    // the level it had reached (a level never falls back there), and keying on
    // the level would leave the edited item bodiless forever.
    let mut targets: Vec<(ReplicaHandle, u64)> = Vec::new();
    for placement in projection_view(store, collection, ctx.side)
        .with_context(|| format!("Project {} `{collection}`", ctx.side))?
    {
        if placement.status == ReplicaStatus::Tombstone || placement.object.is_some() {
            continue;
        }
        let size = meta_size(&placement.meta).unwrap_or(0) as u64;
        targets.push((placement.handle, size));
    }
    Ok((targets, report))
}

/// Phase 2 — hydrate every body across every collection through **one** global
/// work-stealing pool. Bodies are chunked into largest-first per-collection batches
/// (a batched `UID FETCH` is within one selected collection), the biggest batches
/// queued first for a global largest-first order, and work-stolen across the
/// connections: a worker finishing one collection's last batch immediately steals the
/// next collection's — `select_cached` re-SELECTs across the boundary — so no
/// connection idles at a collection edge. Bodies stream into the blob store; the
/// fetched items are cached by `(collection, handle)` for Phase 3.
fn phase2_hydrate(
    plans: &[MailboxPlan],
    ctxs: &mut [SideCtx],
    blobs: &PimdirBlobs,
) -> Result<HashMap<FetchKey, ReplicaFetchedItem>> {
    // (max_size, collection, handles) per batch; sort biggest-first, then queue.
    let mut batches: Vec<(u64, String, Vec<ReplicaHandle>)> = Vec::new();
    let mut total_bodies = 0usize;
    for (collection, targets) in plans {
        let mut sorted = targets.clone();
        sorted.sort_by_key(|(_, size)| Reverse(*size));
        for chunk in sorted.chunks(BATCH_SIZE) {
            let max_size = chunk.iter().map(|(_, size)| *size).max().unwrap_or(0);
            let handles: Vec<ReplicaHandle> = chunk.iter().map(|(h, _)| h.clone()).collect();
            total_bodies += handles.len();
            batches.push((max_size, collection.clone(), handles));
        }
    }
    if total_bodies == 0 {
        return Ok(HashMap::new());
    }
    batches.sort_by_key(|(max_size, ..)| Reverse(*max_size));

    let queue: SegQueue<(String, Vec<ReplicaHandle>)> = SegQueue::new();
    for (_, collection, handles) in batches {
        queue.push((collection, handles));
    }

    // The kind is a property of the backend, so every connection of this side
    // reports the same one; resolve it once here rather than per batch, before
    // the workers borrow the contexts.
    let kind = ctxs
        .first_mut()
        .map(|ctx| resolve_kind(&mut ctx.pool))
        .unwrap_or(Kind::Mail);

    let cache: Mutex<HashMap<FetchKey, ReplicaFetchedItem>> =
        Mutex::new(HashMap::with_capacity(total_bodies));
    let failure: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let stop = AtomicBool::new(false);
    let done = AtomicUsize::new(0);
    let s = Spinner::start(format!("Downloading 0% (0/{total_bodies})"));

    let queue_ref = &queue;
    let cache_ref = &cache;
    let failure_ref = &failure;
    let stop_ref = &stop;
    let done_ref = &done;
    let s_ref = &s;

    thread::scope(|scope| {
        for ctx in ctxs.iter_mut() {
            scope.spawn(move || {
                let on_body = || {
                    let n = done_ref.fetch_add(1, Ordering::Relaxed) + 1;
                    let percent = (n * 100).checked_div(total_bodies).unwrap_or(100);
                    s_ref.set_message(format!("Downloading {percent}% ({n}/{total_bodies})"));
                };
                while !stop_ref.load(Ordering::Relaxed) {
                    let Some((collection, handles)) = queue_ref.pop() else {
                        break;
                    };
                    match hydrate_batch(
                        kind,
                        ctx.pool.primary(),
                        &collection,
                        &handles,
                        blobs,
                        Some(&on_body),
                    ) {
                        Ok(items) => {
                            let mut cache = cache_ref.lock().unwrap();
                            for item in items {
                                cache.insert((collection.clone(), item.handle.0.clone()), item);
                            }
                        }
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
    let cache = cache.into_inner().unwrap();
    s.success(format!("Downloaded {} item(s)", cache.len()));
    Ok(cache)
}

/// Phase 3 — apply the pre-fetched bodies to the index, one collection at a time,
/// with no network: drive io-replica's `Full` upgrade over a [`CachedFetchRemote`]
/// that serves each body from the Phase-2 cache (a miss falls back to a real
/// fetch). Only store writes happen here — fast, single writer.
fn phase3_apply(
    plans: &[MailboxPlan],
    ctx: &mut SideCtx,
    store: &mut PimdirStore,
    blobs: &PimdirBlobs,
    cache: &HashMap<FetchKey, ReplicaFetchedItem>,
) -> Result<()> {
    let total = plans.len();
    let s = Spinner::start(format!("Writing (0/{total})"));
    for (index, (collection, _)) in plans.iter().enumerate() {
        if let Err(err) = apply_full(collection, ctx, store, blobs, cache) {
            warn!("`{collection}` write error: {err:#}");
        }
        s.set_message(format!("Writing ({}/{total})", index + 1));
    }
    s.success(format!("Wrote {total} collection(s)"));
    Ok(())
}

/// Raises every not-yet-`Full` item of `collection` to `Full` from the pre-fetch
/// cache (blobs already on disk), so the retained store holds each body for the
/// app to read offline.
fn apply_full(
    collection: &str,
    ctx: &mut SideCtx,
    store: &mut PimdirStore,
    blobs: &PimdirBlobs,
    cache: &HashMap<FetchKey, ReplicaFetchedItem>,
) -> Result<()> {
    let handles: Vec<ReplicaHandle> = projection_view(store, collection, ctx.side)
        .with_context(|| format!("Project {} `{collection}`", ctx.side))?
        .into_iter()
        .filter(|p| p.status != ReplicaStatus::Tombstone && p.level < ReplicaLevel::Full)
        .map(|p| p.handle)
        .collect();
    if handles.is_empty() {
        return Ok(());
    }
    let fallback = PimRemote::new(&mut ctx.pool, blobs.clone());
    let mut remote = CachedFetchRemote::new(cache, fallback);
    drive(
        store,
        &mut remote,
        ReplicaUpgrade::new(collection.to_string(), handles, ReplicaTier::Full),
    )
    .with_context(|| format!("Apply bodies `{collection}`"))?;
    Ok(())
}

/// A `handle → flags` snapshot of a side's items, taken before a pull so its
/// remote flag changes can be diffed out of the sync's per-item events.
fn flag_snapshot(
    store: &PimdirStore,
    collection: &str,
    side: Side,
) -> Result<HashMap<String, ReplicaFlags>> {
    Ok(load_side(store, collection)
        .with_context(|| format!("Load {side} `{collection}`"))?
        .into_iter()
        .map(|placement| (placement.handle.0, placement.flags))
        .collect())
}

/// Itemizes the remote-originated changes a pull applied — flag changes and
/// removals on already-synced items — into the report. The pull applies them
/// silently (the item reads `Clean` afterwards), so they are recovered from the
/// sync's per-item `events`: a `FlagsChanged` is diffed against the pre-pull
/// snapshot into add/remove hunks, a `Vanished` becomes a delete. (A new remote
/// item is an `Added` event but is already reported as a `Fetch` by the pull
/// plan, so it is not re-itemized here.)
fn itemize_pulled(
    events: &[ReplicaEvent],
    before: &HashMap<String, ReplicaFlags>,
    store: &PimdirStore,
    collection: &str,
    side: Side,
    report: &mut SyncReport,
) -> Result<()> {
    let after = flag_snapshot(store, collection, side)?;
    for event in events {
        match event {
            ReplicaEvent::FlagsChanged(handle) => {
                let old = before.get(&handle.0).cloned().unwrap_or_default();
                let Some(new) = after.get(&handle.0) else {
                    continue;
                };
                let (added, removed) = flag_diff(&old, new);
                if !added.is_empty() {
                    report.item.patch.push(PatchEntry::new(
                        ItemHunk::AddFlags {
                            side,
                            collection: collection.to_string(),
                            id: handle.0.clone(),
                            flags: added,
                            content_key: 0,
                        },
                        None,
                    ));
                }
                if !removed.is_empty() {
                    report.item.patch.push(PatchEntry::new(
                        ItemHunk::RemoveFlags {
                            side,
                            collection: collection.to_string(),
                            id: handle.0.clone(),
                            flags: removed,
                            content_key: 0,
                        },
                        None,
                    ));
                }
            }
            ReplicaEvent::Vanished(handle) => {
                report.item.patch.push(PatchEntry::new(
                    ItemHunk::Delete {
                        side,
                        collection: collection.to_string(),
                        id: handle.0.clone(),
                        content_key: 0,
                    },
                    None,
                ));
            }
            // Content diverged on both sides. The engine left the placement
            // conflicted rather than picking a winner; surface it so it is not
            // silently stuck, and keep surfacing it on every run until someone
            // resolves it with an edit.
            ReplicaEvent::Conflicted(handle) => {
                report.conflicts.push(ItemConflict {
                    side,
                    collection: collection.to_string(),
                    id: handle.0.clone(),
                });
            }
            _ => {}
        }
    }
    Ok(())
}

/// Itemizes one side's pending projection into the report (the local→remote work
/// a flag/move/delete/compose staged), reusing the shared placement mapping.
fn itemize_single(
    collection: &str,
    store: &PimdirStore,
    ctx: &SideCtx,
    report: &mut SyncReport,
) -> Result<()> {
    let view = projection_view(store, collection, ctx.side)
        .with_context(|| format!("Project {} `{collection}`", ctx.side))?;
    for placement in view {
        for hunk in placement_hunks(ctx.side, collection, &placement) {
            report.item.patch.push(PatchEntry::new(hunk, None));
        }
    }
    Ok(())
}

/// Reports the pull plan of a one-source local sync (dry run): each not-yet-Full,
/// non-tombstone item is a body a real run would fetch into the store.
fn itemize_fetches(
    collection: &str,
    store: &PimdirStore,
    side: Side,
    report: &mut SyncReport,
) -> Result<()> {
    let view = projection_view(store, collection, side)
        .with_context(|| format!("Project {side} `{collection}`"))?;
    for placement in view {
        if placement.status == ReplicaStatus::Tombstone || placement.level >= ReplicaLevel::Full {
            continue;
        }
        let id = placement
            .link_id
            .as_ref()
            .map(|l| l.0.clone())
            .unwrap_or_else(|| placement.handle.0.clone());
        report.item.patch.push(PatchEntry::new(
            ItemHunk::Fetch {
                side,
                collection: collection.to_string(),
                id,
                content_key: 0,
            },
            None,
        ));
    }
    Ok(())
}

/// Reconciles one collection: a first per-side reconcile that resolves link ids and
/// pulls both servers into the hub, a hydration of the bodies about to cross,
/// then extra passes that push the projected propagations until quiescent.
#[allow(clippy::too_many_arguments)]
fn sync_mailbox(
    collection: &str,
    media_type: &str,
    left: &mut SideCtx,
    right: &mut SideCtx,
    left_store: &mut PimdirStore,
    right_store: &mut PimdirStore,
    blobs: &PimdirBlobs,
    dry_run: bool,
    relay: bool,
    progress: &MailboxProgress,
    report: &mut SyncReport,
) -> Result<()> {
    // NOTE: declare the collection's media type up front so the store is
    // self-describing; one call suffices (both handles share the database), and
    // the sync's lazy collection creation never clobbers it.
    left_store
        .ensure_collection(collection, media_type)
        .with_context(|| format!("Declare kind for `{collection}`"))?;

    // NOTE: Round one — pull both servers into the shared hub and resolve link
    // ids so items pair up across the sides.
    reconcile_pass(
        collection,
        left,
        right,
        left_store,
        right_store,
        blobs,
        dry_run,
    )?;
    propagate(
        collection,
        left,
        right,
        left_store,
        right_store,
        blobs,
        relay,
        progress,
    )?;

    // NOTE: Itemize the pending cross-side work from the projection — identical
    // for a dry run (which stops here) and a real run (which pushes it below).
    // The projection reads the whole hub, so either handle serves it.
    itemize(collection, left_store, left, right, report)?;
    if dry_run {
        return Ok(());
    }

    // NOTE: Push the propagations and settle. Each pass projects the hub as
    // dirty/created/tombstone for the lagging side, whose sync pushes it.
    for _ in 0..MAX_EXTRA_PASSES {
        let progressed = reconcile_pass(
            collection,
            left,
            right,
            left_store,
            right_store,
            blobs,
            dry_run,
        )?;
        let propagated = propagate(
            collection,
            left,
            right,
            left_store,
            right_store,
            blobs,
            relay,
            progress,
        )?;
        if !progressed && propagated == 0 {
            break;
        }
    }

    Ok(())
}

/// Propagates the collection's cross-side copies, either by retaining (hydrating the
/// body into the store, then the projection pushes it) or by relaying (streaming
/// the body server-to-server, the store keeping only the spine). Returns how many
/// bodies were moved.
#[allow(clippy::too_many_arguments)]
fn propagate(
    collection: &str,
    left: &mut SideCtx,
    right: &mut SideCtx,
    left_store: &mut PimdirStore,
    right_store: &mut PimdirStore,
    blobs: &PimdirBlobs,
    relay: bool,
    progress: &MailboxProgress,
) -> Result<usize> {
    if relay {
        relay_copies(collection, left, right, left_store, progress)
    } else {
        hydrate_copies(
            collection,
            left,
            right,
            left_store,
            right_store,
            blobs,
            progress,
        )
    }
}

/// One cross-copy body to relay: its holding side + fetch handle, the exact octet
/// length (from the item's `v:1` meta `size`, so the target APPEND is
/// length-prefixed without buffering the body), the flags and the Message-ID.
struct RelayTarget {
    holding: Side,
    handle: ReplicaHandle,
    size: usize,
    flags: Vec<Flag>,
    message_id: Option<String>,
}

/// Reads the relay targets from the hub: an item held by exactly one side, not
/// yet on the other (never hydrated — no stored object), whose far side may
/// create it.
fn relay_targets(
    store: &PimdirStore,
    collection: &str,
    left_creates: bool,
    right_creates: bool,
) -> Result<Vec<RelayTarget>> {
    let hub = store.load_hub(collection)?;
    let mut out = Vec::new();
    for (link, item) in &hub.items {
        if item.deleted || item.object.is_some() || item.sources.len() != 1 {
            continue;
        }
        let (held, binding) = item.sources.iter().next().expect("one source");
        let holding = if *held == source_id(Side::Left) {
            Side::Left
        } else {
            Side::Right
        };
        let target_creates = match holding {
            Side::Left => right_creates,
            Side::Right => left_creates,
        };
        if !target_creates {
            continue;
        }
        let Some(size) = meta_size(&item.meta) else {
            warn!(
                "relay skips `{}` in `{collection}`: no size in meta",
                link.0
            );
            continue;
        };
        out.push(RelayTarget {
            holding,
            handle: binding.handle.clone(),
            size,
            flags: to_email_flag_set(&item.flags).into_iter().collect(),
            message_id: link.0.strip_prefix("mid:").map(str::to_string),
        });
    }
    Ok(out)
}

/// The `size` (octet length) of a `v:1` mail meta, when present.
fn meta_size(meta: &Option<ReplicaMeta>) -> Option<usize> {
    let raw = meta.as_ref()?;
    let value: serde_json::Value = serde_json::from_str(&raw.0).ok()?;
    value.get("size")?.as_u64().map(|n| n as usize)
}

/// Streams each cross-copy body directly from its holding side to the other,
/// through a bounded pipe — the store keeps only the spine (the target's next
/// enumerate binds the relayed message; its body is never stored). Returns how
/// many were relayed.
fn relay_copies(
    collection: &str,
    left: &mut SideCtx,
    right: &mut SideCtx,
    store: &mut PimdirStore,
    progress: &MailboxProgress,
) -> Result<usize> {
    let targets = relay_targets(
        store,
        collection,
        left.perms.item.create,
        right.perms.item.create,
    )?;
    let total = targets.len();
    let mut count = 0;
    for target in targets {
        let (holding_pool, target_pool) = match target.holding {
            Side::Left => (&mut left.pool, &mut right.pool),
            Side::Right => (&mut right.pool, &mut left.pool),
        };
        relay_one(holding_pool, target_pool, collection, &target)
            .with_context(|| format!("Relay `{}` in `{collection}`", target.handle.0))?;
        count += 1;
        progress.tick(count, total);
    }
    Ok(count)
}

/// Relays one message: a worker thread streams the fetch into the bounded pipe
/// while this thread streams the pipe into the length-prefixed target append, so
/// the body flows source→target without ever being held whole or stored.
fn relay_one(
    holding_pool: &mut Pool,
    target_pool: &mut Pool,
    collection: &str,
    target: &RelayTarget,
) -> Result<()> {
    let (writer, reader) = pipe::bounded(256 * 1024);
    let holding = holding_pool.primary();
    let dest = target_pool.primary();
    let handle = target.handle.0.clone();

    let (fetch, append) = std::thread::scope(|scope| {
        let fetch = scope.spawn(move || holding.get_item_stream(collection, &handle, writer));
        let append = dest.add_item_stream(
            collection,
            &target.flags,
            reader,
            target.size,
            target.message_id.as_deref(),
        );
        (fetch.join().unwrap(), append)
    });
    fetch.context("relay fetch")?;
    append.context("relay append")?;
    Ok(())
}

/// One per-side reconcile round: sync each side against its server (pull +
/// push), then resolve any freshly probed placement to `Meta` so its link id is
/// known and it joins the hub. Returns whether either side pulled or pushed.
#[allow(clippy::too_many_arguments)]
fn reconcile_pass(
    collection: &str,
    left: &mut SideCtx,
    right: &mut SideCtx,
    left_store: &mut PimdirStore,
    right_store: &mut PimdirStore,
    blobs: &PimdirBlobs,
    dry_run: bool,
) -> Result<bool> {
    let left_report = sync_side_rebuilding(
        collection,
        left,
        left_store,
        blobs,
        !dry_run && left.writable(),
    )?;
    upgrade_probed(collection, left, left_store, blobs)?;
    let right_report = sync_side_rebuilding(
        collection,
        right,
        right_store,
        blobs,
        !dry_run && right.writable(),
    )?;
    upgrade_probed(collection, right, right_store, blobs)?;
    Ok(moved(&left_report) || moved(&right_report))
}

/// Whether a side's sync changed anything (pulled or the remote accepted a
/// push), used to detect convergence.
fn moved(report: &ReplicaSyncReport) -> bool {
    report.pulled > 0 || report.pushed > 0
}

/// Runs one side's sync, then (IMAP only) the handle-space rebuild guard:
/// the stored checkpoint's UIDVALIDITY is read before and after the sync;
/// a change means the server renumbered every UID, so io-replica's rekey
/// re-enumerates the new handle space and carries cached bodies, summaries
/// and pending state over by link id, and its write batch lands through
/// [`PimdirStore::write_rekeyed`] so `collections.generation` bumps
/// atomically with the rebuild (a frontend derives its epoch — an IMAP
/// UIDVALIDITY — from it alone). Ordinary syncs and full resyncs never
/// bump. Graph sides never rebuild: their message ids survive a delta
/// reset (see `crate::msgraph::client`).
///
/// NOTE: the guard is pre/post within one run (neverest keeps no
/// per-collection state beside the store). A crash between the sync's
/// checkpoint write and the rekey loses the pre value, so that window can
/// miss one generation bump; content still converges through link ids on
/// the next sync.
fn sync_side_rebuilding(
    collection: &str,
    ctx: &mut SideCtx,
    store: &mut PimdirStore,
    blobs: &PimdirBlobs,
    push: bool,
) -> Result<ReplicaSyncReport> {
    let pre = stored_epoch(ctx.pool.primary(), store, collection)?;

    let report = sync_side(collection, ctx, store, blobs, push)?;

    if let Some(pre) = pre
        && let Some(post) = stored_epoch(ctx.pool.primary(), store, collection)?
        && post != pre
    {
        info!("handle-space epoch of `{collection}` changed, rebuilding the collection");
        let (rekey, generation) = rebuild_collection(collection, ctx, store, blobs)?;
        info!(
            "`{collection}` rebuilt under generation {generation}: {} carried, {} pulled, {} pending dropped",
            rekey.rekeyed, rekey.pulled, rekey.dropped
        );
    }

    Ok(report)
}

/// The handle-space epoch carried by `collection`'s stored checkpoint, `None`
/// when there is no checkpoint or the backend has no such notion.
///
/// The checkpoint bytes are the backend's own encoding, so the backend decodes
/// them ([`Client::handle_space_epoch`]); the driver only compares the number
/// it gets back, and knows nothing about IMAP's UIDVALIDITY.
fn stored_epoch(client: &Client, store: &PimdirStore, collection: &str) -> Result<Option<u64>> {
    let loaded = store
        .load(&ReplicaCollectionId(collection.to_string()))
        .map_err(|err| anyhow!("Load `{collection}` checkpoint error: {err}"))?;
    Ok(loaded
        .checkpoint
        .as_ref()
        .and_then(|checkpoint| client.handle_space_epoch(&checkpoint.0)))
}

/// Drives io-replica's rekey over the side's remote, routing its rebuild
/// write batch through [`PimdirStore::write_rekeyed`] instead of the
/// plain storage seam, so "the ids you cached are void" (the generation
/// bump) commits atomically with the rebuild that voided them. Returns
/// the rekey report and the new generation.
fn rebuild_collection(
    collection: &str,
    ctx: &mut SideCtx,
    store: &mut PimdirStore,
    blobs: &PimdirBlobs,
) -> Result<(ReplicaRekeyReport, i64)> {
    let mut remote = PimRemote::new(&mut ctx.pool, blobs.clone());
    drive_rekey(store, &mut remote, collection)
}

/// The generic rekey pump behind [`rebuild_collection`], seam-typed so a
/// test can drive it over a scripted remote.
fn drive_rekey<R>(
    store: &mut PimdirStore,
    remote: &mut R,
    collection: &str,
) -> Result<(ReplicaRekeyReport, i64)>
where
    R: ReplicaRemote,
    R::Error: std::fmt::Display,
{
    let mut coroutine = ReplicaRekey::new(collection.to_string());
    let mut arg: Option<ReplicaArg> = None;
    let mut generation: Option<i64> = None;

    loop {
        match coroutine.resume(arg.take()) {
            ReplicaCoroutineState::Complete(Ok(report)) => {
                // NOTE: the rekey emits exactly one write batch, so exactly
                // one generation bump lands.
                let generation = generation.context("Rekey completed without a write")?;
                return Ok((report, generation));
            }
            ReplicaCoroutineState::Complete(Err(err)) => {
                return Err(anyhow!("Rekey engine error: {err}"));
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsLoad(collection)) => {
                let loaded = store
                    .load(&collection)
                    .map_err(|err| anyhow!("Storage load error: {err}"))?;
                arg = Some(ReplicaArg::Load(loaded));
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsLookupObject(links)) => {
                let known = store
                    .lookup_objects(&links)
                    .map_err(|err| anyhow!("Storage lookup error: {err}"))?;
                arg = Some(ReplicaArg::LookupObject(known));
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsEnumerate { collection, cursor }) => {
                let snapshot = remote
                    .enumerate(&collection, cursor)
                    .map_err(|err| anyhow!("Remote enumerate error: {err}"))?;
                arg = Some(ReplicaArg::Enumerate(snapshot));
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsFetch {
                collection,
                handles,
                tier,
            }) => {
                let items = remote
                    .fetch(&collection, handles, tier)
                    .map_err(|err| anyhow!("Remote fetch error: {err}"))?;
                arg = Some(ReplicaArg::Fetch(items));
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsWrite(ops)) => {
                generation = Some(
                    store
                        .write_rekeyed(collection, ops)
                        .map_err(|err| anyhow!("Rekeyed write error: {err}"))?,
                );
                arg = Some(ReplicaArg::Write);
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsPush { .. }) => {
                bail!("Rekey asked for a push");
            }
        }
    }
}

/// Runs one side's `sync` against its server and returns its report.
fn sync_side(
    collection: &str,
    ctx: &mut SideCtx,
    store: &mut PimdirStore,
    blobs: &PimdirBlobs,
    push: bool,
) -> Result<ReplicaSyncReport> {
    let opts = ReplicaSyncOptions {
        push,
        rights: ctx.push_rights(),
        ..Default::default()
    };
    let mut remote = PimRemote::new(&mut ctx.pool, blobs.clone());
    drive(
        store,
        &mut remote,
        ReplicaSync::new(collection.to_string(), opts),
    )
    .with_context(|| format!("Sync {} `{collection}`", ctx.side))
}

/// Raises every freshly probed placement to the tier its kind resolves at
/// ([`Kind::probe_tier`]) so its link id and summary are known and it enters
/// the hub: `Meta` for mail, whose envelope carries the identity, `Full` for
/// a kind whose body is the only thing that does.
fn upgrade_probed(
    collection: &str,
    ctx: &mut SideCtx,
    store: &mut PimdirStore,
    blobs: &PimdirBlobs,
) -> Result<()> {
    let probed: Vec<ReplicaHandle> = load_side(store, collection)
        .with_context(|| format!("Load {} `{collection}`", ctx.side))?
        .into_iter()
        .filter(|p| p.level == ReplicaLevel::Probed && p.status != ReplicaStatus::Tombstone)
        .map(|p| p.handle)
        .collect();
    if probed.is_empty() {
        return Ok(());
    }
    let tier = resolve_kind(&mut ctx.pool).probe_tier();
    let mut remote = PimRemote::new(&mut ctx.pool, blobs.clone());
    drive(
        store,
        &mut remote,
        ReplicaUpgrade::new(collection.to_string(), probed, tier),
    )
    .with_context(|| format!("Upgrade probed {} `{collection}`", ctx.side))?;
    Ok(())
}

/// Hydrates (to `Full`) the bodies of items held by only one side that the other
/// side may receive, so the hub holds the body and its next projection stages
/// the copy. Returns how many bodies were fetched.
fn hydrate_copies(
    collection: &str,
    left: &mut SideCtx,
    right: &mut SideCtx,
    left_store: &mut PimdirStore,
    right_store: &mut PimdirStore,
    blobs: &PimdirBlobs,
    progress: &MailboxProgress,
) -> Result<usize> {
    // NOTE: the hub is shared, so either handle reports the same targets.
    let targets = hydration_targets(
        left_store,
        collection,
        left.perms.item.create,
        right.perms.item.create,
    )
    .with_context(|| format!("Hydration targets `{collection}`"))?;
    if targets.is_empty() {
        return Ok(0);
    }

    // NOTE: the holding side owns the bodies; batch each side's targets into one
    // Full upgrade so its fetch streams them concurrently, largest-first, instead
    // of one heavy item stalling the rest.
    let mut left_handles = Vec::new();
    let mut right_handles = Vec::new();
    for (side, handle) in &targets {
        match side {
            Side::Left => left_handles.push(handle.clone()),
            Side::Right => right_handles.push(handle.clone()),
        }
    }
    let total = targets.len();
    // One counter over both sides so the tick reads `fetched i/total` end to end.
    let done = AtomicUsize::new(0);
    let tick = || progress.tick(done.fetch_add(1, Ordering::Relaxed) + 1, total);

    if !left_handles.is_empty() {
        // Cross-side hydration orders by UID (empty size map); largest-first is
        // the local-sync retain path's concern.
        let mut remote =
            PimRemote::with_progress(&mut left.pool, blobs.clone(), &tick, HashMap::new());
        drive(
            left_store,
            &mut remote,
            ReplicaUpgrade::new(collection.to_string(), left_handles, ReplicaTier::Full),
        )
        .with_context(|| format!("Hydrate bodies left `{collection}`"))?;
    }
    if !right_handles.is_empty() {
        let mut remote =
            PimRemote::with_progress(&mut right.pool, blobs.clone(), &tick, HashMap::new());
        drive(
            right_store,
            &mut remote,
            ReplicaUpgrade::new(collection.to_string(), right_handles, ReplicaTier::Full),
        )
        .with_context(|| format!("Hydrate bodies right `{collection}`"))?;
    }
    Ok(total)
}

/// Itemizes the pending cross-side work into the report by reading each side's
/// hub projection: a `Created` placement is a copy in, a `Dirty` one a flag
/// change, a `Tombstone` a delete.
fn itemize(
    collection: &str,
    store: &PimdirStore,
    left: &SideCtx,
    right: &SideCtx,
    report: &mut SyncReport,
) -> Result<()> {
    for ctx in [left, right] {
        let view = projection_view(store, collection, ctx.side)
            .with_context(|| format!("Project {} `{collection}`", ctx.side))?;
        for placement in view {
            for hunk in placement_hunks(ctx.side, collection, &placement) {
                report.item.patch.push(PatchEntry::new(hunk, None));
            }
        }
    }
    Ok(())
}

/// Maps a projected placement to its report hunks (a flag change can surface as
/// both an add and a remove).
fn placement_hunks(side: Side, collection: &str, placement: &ReplicaPlacement) -> Vec<ItemHunk> {
    match placement.status {
        ReplicaStatus::Created => {
            let link = placement
                .link_id
                .as_ref()
                .map(|l| l.0.clone())
                .unwrap_or_default();
            vec![ItemHunk::Copy {
                source_side: side.other(),
                target_side: side,
                collection: collection.to_string(),
                source_id: link.clone(),
                flags: to_email_flag_set(&placement.flags),
                content_key: content_key(&link),
            }]
        }
        ReplicaStatus::Tombstone => vec![ItemHunk::Delete {
            side,
            collection: collection.to_string(),
            id: placement.handle.0.clone(),
            content_key: 0,
        }],
        ReplicaStatus::Dirty => {
            let base = placement
                .base
                .as_ref()
                .map(|b| b.flags.clone())
                .unwrap_or_default();
            let (added, removed) = flag_diff(&base, &placement.flags);
            let mut hunks = Vec::new();
            if !added.is_empty() {
                hunks.push(ItemHunk::AddFlags {
                    side,
                    collection: collection.to_string(),
                    id: placement.handle.0.clone(),
                    flags: added,
                    content_key: 0,
                });
            }
            if !removed.is_empty() {
                hunks.push(ItemHunk::RemoveFlags {
                    side,
                    collection: collection.to_string(),
                    id: placement.handle.0.clone(),
                    flags: removed,
                    content_key: 0,
                });
            }
            // `Dirty` covers a flag change *and* a body edit, so compare the
            // body pointer against the base's too: a mutable-content item whose
            // object moved is a pending in-place update. Mail never trips this
            // (its bodies are immutable, so the pointer only moves when the item
            // does), which is why the flag diff alone sufficed until now.
            let base_object = placement.base.as_ref().and_then(|b| b.object.as_ref());
            if placement.object.as_ref() != base_object {
                hunks.push(ItemHunk::Update {
                    side,
                    collection: collection.to_string(),
                    id: placement.handle.0.clone(),
                    content_key: 0,
                });
            }
            hunks
        }
        ReplicaStatus::Clean | ReplicaStatus::Conflict => Vec::new(),
    }
}

fn side_ctx<'a>(side: Side, left: &'a mut SideCtx, right: &'a mut SideCtx) -> &'a mut SideCtx {
    match side {
        Side::Left => left,
        Side::Right => right,
    }
}

fn list_collections(client: &mut Client) -> Result<HashSet<String>> {
    Ok(client
        .list_collections(false)
        .context("List collections error")?
        .into_iter()
        .map(|m| m.name)
        .collect())
}

fn filter_mailboxes(collections: &HashSet<String>, filter: &CollectionFilter) -> BTreeSet<String> {
    let matches = |name: &str, list: &[String]| list.iter().any(|f| f.eq_ignore_ascii_case(name));
    collections
        .iter()
        .filter(|name| match filter {
            CollectionFilter::All => true,
            CollectionFilter::Include(list) => matches(name, list),
            CollectionFilter::Exclude(list) => !matches(name, list),
        })
        .cloned()
        .collect()
}

/// Create-only collection diff: make each side hold the union of filtered
/// collections, gated by the target side's create permission.
fn diff_mailboxes(
    left: &BTreeSet<String>,
    right: &BTreeSet<String>,
    left_ctx: &SideCtx,
    right_ctx: &SideCtx,
) -> Vec<CollectionHunk> {
    let mut hunks = Vec::new();
    for collection in left.difference(right) {
        if right_ctx.perms.collection.create {
            hunks.push(CollectionHunk::Create {
                side: Side::Right,
                collection: collection.clone(),
            });
        }
    }
    for collection in right.difference(left) {
        if left_ctx.perms.collection.create {
            hunks.push(CollectionHunk::Create {
                side: Side::Left,
                collection: collection.clone(),
            });
        }
    }
    let _ = (left_ctx.side, right_ctx.side);
    hunks
}

fn apply_mailbox_hunk(
    hunk: &CollectionHunk,
    left: &mut SideCtx,
    right: &mut SideCtx,
) -> Result<()> {
    match hunk {
        CollectionHunk::Create { side, collection } => {
            side_ctx(*side, left, right)
                .pool
                .primary()
                .create_collection(collection)
                .with_context(|| format!("Create collection `{collection}` on {side}"))?;
        }
        CollectionHunk::Delete { side, collection } => {
            side_ctx(*side, left, right)
                .pool
                .primary()
                .delete_collection(collection)
                .with_context(|| format!("Delete collection `{collection}` on {side}"))?;
        }
    }
    Ok(())
}

fn to_email_flag_set(flags: &ReplicaFlags) -> BTreeSet<Flag> {
    let Some(flags) = flags.known() else {
        return BTreeSet::new();
    };

    flags.iter().map(|s| Flag::from_raw(s.clone())).collect()
}

/// What `new` gained over `old`, and what it lost, as the report renders
/// them. An unknown set holds no markers to compare with, so it reads as
/// empty: nothing is reported added or removed against a side nobody read.
fn flag_diff(old: &ReplicaFlags, new: &ReplicaFlags) -> (BTreeSet<Flag>, BTreeSet<Flag>) {
    let empty = BTreeSet::new();
    let old = old.known().unwrap_or(&empty);
    let new = new.known().unwrap_or(&empty);
    let diff = |from: &BTreeSet<String>, to: &BTreeSet<String>| {
        from.difference(to)
            .map(|flag| Flag::from_raw(flag.clone()))
            .collect()
    };

    (diff(new, old), diff(old, new))
}

/// A stable-ish u64 content key for the report DTO (display never shows it; it
/// only needs to be internally consistent).
fn content_key(link: &str) -> u64 {
    let mut acc: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in link.bytes() {
        acc ^= byte as u64;
        acc = acc.wrapping_mul(0x0000_0100_0000_01b3);
    }
    acc
}

/// Drains every collection's pending frontend actions into the store
/// before the sync (spec: neverest is the store's sole owner). Each
/// collection is exactly-once applied by io-pimdir's
/// [`drain_collection`](PimdirStore::drain_collection); permanently bad
/// actions park (reported as warnings until repaired), a transient
/// failure leaves the collection queued for the next run and is only
/// warned — the sync itself still runs offline-first.
fn drain_queues(store: &mut PimdirStore, report: &mut SyncReport) {
    let collections = match store.queued_collections() {
        Ok(collections) => collections,
        Err(err) => {
            warn!("cannot list queued collections: {err}");
            return;
        }
    };

    for collection in collections {
        match store.drain_collection(&collection) {
            Ok(drained) => {
                if drained.applied > 0 || drained.parked > 0 {
                    info!(
                        "drained {} queued action(s) in `{collection}` ({} parked)",
                        drained.applied, drained.parked
                    );
                }
                if drained.applied > 0 {
                    report.drained.push(DrainedQueue {
                        collection: collection.clone(),
                        applied: drained.applied,
                    });
                }
            }
            Err(err) => warn!("drain of `{collection}` failed, actions stay queued: {err}"),
        }
    }

    match store.parked_actions() {
        Ok(parked) => {
            for action in parked {
                report.parked.push(ParkedQueueAction {
                    id: action.id,
                    collection: action.collection,
                    action: action.action,
                    producer: action.producer,
                    error: action.error,
                });
            }
        }
        Err(err) => warn!("cannot list parked actions: {err}"),
    }
}

/// Performs the queue's `submit` intents, the half the store's own drain
/// leaves pending because performing it is a capability, not a mutation
/// (spec: an owner that cannot perform an action kind skips the row).
///
/// Each intent goes through the first side offering a send channel: its own
/// `smtp` table when it carries one, else its native send (the Graph
/// sendMail action). A sent intent is acknowledged (`drop_action`), which
/// releases its body's pin; a permanent failure parks the row with its
/// error; a transient one leaves it pending for the next run. Without a
/// channel at all the intents stay pending with a warning: they are never
/// parked, since another build (or another host) can perform them.
fn drain_submits(
    account_config: &AccountConfig,
    sides: &mut [&mut SideCtx],
    store: &mut PimdirStore,
    blobs: &PimdirBlobs,
    report: &mut SyncReport,
) {
    let intents = match submit::pending(store) {
        Ok(intents) => intents,
        Err(err) => {
            warn!("cannot read the queued submit intents: {err:#}");
            return;
        }
    };
    if intents.is_empty() {
        return;
    }
    info!("performing {} queued submit intent(s)", intents.len());

    #[cfg(any(feature = "smtp", feature = "msgraph"))]
    {
        let Some(mut channel) = open_send_channel(account_config, sides, intents.len()) else {
            return;
        };

        let mut sent = 0;
        for intent in &intents {
            let subject = intent.subject();
            let entry = match submit::send_one(&mut channel, blobs, intent) {
                Ok(()) => {
                    // NOTE: at-least-once, since a crash between the server's
                    // acceptance and this acknowledgement resends next run
                    // (see the module header; dedup is the provider's job).
                    match store.drop_action(intent.id) {
                        Ok(true) => {}
                        Ok(false) => warn!(
                            "submit intent #{} vanished from the queue before its acknowledgement",
                            intent.id
                        ),
                        Err(err) => warn!(
                            "submit intent #{} was sent but could not be acknowledged, it will be resent: {err}",
                            intent.id
                        ),
                    }
                    sent += 1;
                    SubmitEntry {
                        id: intent.id,
                        collection: intent.collection.clone(),
                        subject,
                        error: None,
                        parked: false,
                    }
                }
                Err(failure) => {
                    let error = format!("{:#}", failure.error());
                    let parked = failure.parks();
                    warn!("submit intent #{} failed: {error}", intent.id);
                    // NOTE: `None` bumps the attempt counter and leaves the
                    // row pending for the next run; `Some` parks it.
                    let outcome = parked.then_some(error.as_str());
                    if let Err(err) = store.fail_action(intent.id, outcome) {
                        warn!("cannot record submit intent #{} failure: {err}", intent.id);
                    }
                    SubmitEntry {
                        id: intent.id,
                        collection: intent.collection.clone(),
                        subject,
                        error: Some(error),
                        parked,
                    }
                }
            };
            report.submitted.push(entry);
        }

        channel.close();
        if sent > 0 {
            info!("submitted {sent} of {} queued intent(s)", intents.len());
        }
    }

    #[cfg(not(any(feature = "smtp", feature = "msgraph")))]
    {
        // NOTE: skipped, never parked: parking means permanently
        // unappliable, and a build with a send channel performs these.
        let _ = (account_config, sides, blobs, store, report);
        warn!(
            "this build has no send channel (needs the `smtp` or the `msgraph` cargo feature), {} submit intent(s) stay pending",
            intents.len()
        );
    }
}

/// Reclaims the store's retained (soft-deleted) items older than
/// `store.purge-after`, after the sync and before the report is finalised.
///
/// Unset means never purge, so the sweep does not run at all. It warns
/// rather than fails: a store that cannot be swept is a housekeeping
/// problem, not a reason to fail a run that synced correctly.
fn sweep_retained(
    account_config: &AccountConfig,
    store: &mut PimdirStore,
    report: &mut SyncReport,
) {
    let Some(cutoff) = account_config.store.purge_cutoff(chrono::Utc::now()) else {
        return;
    };

    match store.purge_retained_before(&cutoff) {
        Ok(purged) => {
            if purged.items > 0 {
                info!(
                    "purged {} retained item(s) older than {cutoff}, {} byte(s) reclaimed",
                    purged.items, purged.bytes
                );
            }
            report.purged = Some(PurgedItems {
                items: purged.items,
                bytes: purged.bytes,
            });
        }
        Err(err) => warn!("retention sweep failed, nothing was purged: {err}"),
    }
}

/// Resolves the account's send channel. The channel belongs to a side:
/// its own `smtp` table when it carries one, else its native send (the
/// Graph `sendMail` action). Sides are walked in configuration order, so
/// `left` wins when both offer one. `None` warns and leaves the `queued`
/// intent(s) pending.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
fn open_send_channel<'a>(
    account_config: &AccountConfig,
    sides: &'a mut [&mut SideCtx],
    queued: usize,
) -> Option<submit::SendChannel<'a>> {
    // Decided before any borrow of `sides`, so the Graph arm can hand
    // out the live session of the side it picked.
    let pick = account_config
        .sides()
        .into_iter()
        .zip(0..sides.len())
        .find_map(|((_, side), index)| match &side.smtp {
            Some(smtp) => Some(SendChannelPick::Smtp(smtp)),
            None if side.sends_natively() => Some(SendChannelPick::Native(index)),
            None => None,
        });

    match pick {
        #[cfg(feature = "smtp")]
        Some(SendChannelPick::Smtp(config)) => match submit::connect_smtp(config) {
            Ok(client) => Some(submit::SendChannel::Smtp(client)),
            Err(err) => {
                warn!("cannot open the smtp channel, {queued} intent(s) stay pending: {err:#}");
                None
            }
        },
        #[cfg(not(feature = "smtp"))]
        Some(SendChannelPick::Smtp(_)) => {
            warn!(
                "the side's `smtp` table needs the `smtp` cargo feature, {queued} intent(s) stay pending"
            );
            None
        }
        #[cfg(feature = "msgraph")]
        Some(SendChannelPick::Native(index)) => match sides[index].pool.primary() {
            Client::Msgraph(client) => Some(submit::SendChannel::Graph(client.as_mut())),
            #[allow(unreachable_patterns)]
            _ => {
                warn!("no send channel available, {queued} intent(s) stay pending");
                None
            }
        },
        #[cfg(not(feature = "msgraph"))]
        Some(SendChannelPick::Native(_)) => {
            warn!(
                "the side sends through Microsoft Graph, which needs the `msgraph` cargo feature, {queued} intent(s) stay pending"
            );
            None
        }
        None => {
            warn!("no send channel available, {queued} intent(s) stay pending");
            None
        }
    }
}

/// Which side performs the submit intents, and how: its own SMTP channel,
/// or the live session of the side at that index (a native sender).
///
// NOTE: each variant is consumed by the arm its cargo feature gates, so a
// build with only one of them carries the other unread rather than
// duplicating the walk per feature combination.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
#[allow(dead_code)]
enum SendChannelPick<'a> {
    Smtp(&'a SmtpConfig),
    Native(usize),
}

/// Hydrates every not-yet-`Full`, non-tombstone placement of both sides
/// to `Full` (`store.hydration = "full"`), so the store mirrors every
/// body. The shared object store dedups: the second side's upgrade
/// links an already-stored body by link id without a fetch. Returns how
/// many placements were raised.
#[allow(clippy::too_many_arguments)]
fn hydrate_full_mailbox(
    collection: &str,
    left: &mut SideCtx,
    right: &mut SideCtx,
    left_store: &mut PimdirStore,
    right_store: &mut PimdirStore,
    blobs: &PimdirBlobs,
    progress: &MailboxProgress,
) -> Result<usize> {
    let mut raised = 0;
    for (ctx, store) in [(left, left_store), (right, right_store)] {
        let targets: Vec<ReplicaHandle> = load_side(store, collection)
            .with_context(|| format!("Load {} `{collection}`", ctx.side))?
            .into_iter()
            .filter(|p| p.status != ReplicaStatus::Tombstone && p.level < ReplicaLevel::Full)
            .map(|p| p.handle)
            .collect();
        if targets.is_empty() {
            continue;
        }
        let total = targets.len();
        let done = AtomicUsize::new(0);
        let tick = || progress.tick(done.fetch_add(1, Ordering::Relaxed) + 1, total);
        let mut remote =
            PimRemote::with_progress(&mut ctx.pool, blobs.clone(), &tick, HashMap::new());
        drive(
            store,
            &mut remote,
            ReplicaUpgrade::new(collection.to_string(), targets, ReplicaTier::Full),
        )
        .with_context(|| format!("Hydrate all bodies {} `{collection}`", ctx.side))?;
        raised += total;
    }
    Ok(raised)
}

/// Builds a throwaway copy of the pimdir store for a dry run (so no checkpoint
/// advances and no server is written): the SQLite database, its sidecar files
/// and the blob directory.
fn dry_run_replica(real_dir: &Path) -> Result<PathBuf> {
    let mut tmp = std::env::temp_dir();
    tmp.push(format!("neverest-dry-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp)
        .with_context(|| format!("Create dry-run dir `{}` error", tmp.display()))?;

    if real_dir.exists() {
        copy_dir_all(real_dir, &tmp)
            .with_context(|| format!("Copy `{}` for dry-run", real_dir.display()))?;
    }
    Ok(tmp)
}

/// Recursively copies `src`'s contents into `dst` (created on demand).
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use io_pimdir::{PimdirProducer, codec::PimdirAction};
    use io_replica::{
        change::{ReplicaChange, ReplicaWriteOp},
        collection::ReplicaCheckpoint,
        object::{ReplicaHash, ReplicaObject},
        placement::{ReplicaBase, ReplicaLinkId, ReplicaSortKey},
        remote::{
            ReplicaFetchedItem, ReplicaPushOutcome, ReplicaPushResult, ReplicaRemoteItem,
            ReplicaRemoteSnapshot,
        },
    };

    use super::*;

    #[test]
    fn a_side_pair_must_agree_on_its_kind() {
        // The kind is never configured — it falls out of each side's backend —
        // so this runtime check is the only thing standing between a mail side
        // and an address-book side sharing one collection.
        let mail = "message/rfc822";

        // Two mail sides: the ordinary case, one kind out.
        assert_eq!(
            pair_kind((Side::Left, mail), (Side::Right, mail)).unwrap(),
            Kind::Mail
        );

        // A media type this build cannot sync is refused, and the error names
        // both the type and which side carried it.
        let err = pair_kind((Side::Left, mail), (Side::Right, "text/vcard"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("text/vcard"), "{err}");
        assert!(err.contains("right"), "{err}");

        let err = pair_kind((Side::Left, ""), (Side::Right, mail))
            .unwrap_err()
            .to_string();
        assert!(err.contains("left"), "{err}");

        // NOTE: the *mismatch* branch (two kinds this build knows, but
        // different ones) cannot be exercised until a second backend lands in
        // phase 4; today every known media type is mail, so an unknown one
        // trips the resolve step first.
    }

    #[test]
    fn the_pre_sync_drain_applies_queued_actions_and_reports_parked_ones() {
        let dir = tempfile::tempdir().unwrap();
        // NOTE: the owner opens (and creates) the store first, as the real
        // driver does; only then can a producer attach.
        let mut store = PimdirStore::open(dir.path(), "left").unwrap();
        store.ensure_collection("INBOX", "message/rfc822").unwrap();

        // A frontend enqueues an add (body written durably first) and a
        // permanently bad action (unknown seq).
        let blobs = store.blobs();
        let mut writer = blobs.writer().unwrap();
        std::io::Write::write_all(&mut writer, b"Subject: queued\r\n\r\nhello").unwrap();
        let hash = ReplicaHash("cafe0001".into());
        let size = writer.commit(&hash).unwrap();

        let mut producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
        producer
            .enqueue(
                "INBOX",
                &PimdirAction::Add {
                    link_id: Some(ReplicaLinkId("mid:q1@x".into())),
                    flags: ReplicaFlags::from_iter(["\\Seen"]),
                    object: Some(hash),
                    meta: Some(ReplicaMeta(r#"{"v":1,"subject":"queued"}"#.into())),
                    handle: None,
                },
                Some(size),
                "2026-08-07T00:00:00Z",
            )
            .unwrap();
        producer
            .enqueue(
                "INBOX",
                &PimdirAction::Remove { seq: 424242 },
                None,
                "2026-08-07T00:00:01Z",
            )
            .unwrap();
        producer
            .enqueue(
                "INBOX",
                &PimdirAction::SetFlags {
                    seq: 424243,
                    flags: ReplicaFlags::from_iter(["\\Seen"]),
                },
                None,
                "2026-08-07T00:00:02Z",
            )
            .unwrap();

        let mut report = SyncReport::default();
        drain_queues(&mut store, &mut report);

        // The add applied (staged as a pending creation the sync derives an
        // append push from) and the remove of an absent item is a no-op
        // success; the set-flags on an unknown seq parked.
        assert_eq!(report.drained.len(), 1);
        assert_eq!(report.drained[0].collection, "INBOX");
        assert_eq!(report.drained[0].applied, 2);
        assert_eq!(report.parked.len(), 1);
        assert!(report.parked[0].error.contains("unknown seq"));

        let placements = load_side(&store, "INBOX").unwrap();
        assert_eq!(placements.len(), 1);
        // NOTE: the drained add lands base-less, which the hub projects as a
        // dirty staged creation — exactly what the next sync derives an
        // append push from.
        assert_eq!(placements[0].status, ReplicaStatus::Dirty);
        assert!(placements[0].base.is_none());
        assert_eq!(
            placements[0].link_id.as_ref().map(|l| l.0.as_str()),
            Some("mid:q1@x")
        );

        // The queue is empty (the parked row is not pending) and a second
        // drain applies nothing new but re-reports the parked row.
        let mut second = SyncReport::default();
        drain_queues(&mut store, &mut second);
        assert!(second.drained.is_empty());
        assert_eq!(second.parked.len(), 1);
    }

    /// The stored checkpoint's UIDVALIDITY, read the way the IMAP adapter
    /// encodes it. The rekey guard itself goes through
    /// `Client::handle_space_epoch`; this test has no client, so it decodes
    /// directly — which is also why it is gated on the IMAP feature.
    #[cfg(feature = "imap")]
    fn stored_checkpoint_uid_validity(store: &PimdirStore, collection: &str) -> Option<u32> {
        let loaded = store
            .load(&ReplicaCollectionId(collection.to_string()))
            .unwrap();
        loaded
            .checkpoint
            .as_ref()
            .and_then(|checkpoint| crate::imap::backend::checkpoint_uid_validity(&checkpoint.0))
    }

    /// A scripted remote for the rekey pump: a fixed new spine plus meta
    /// answers, rejecting pushes.
    #[cfg(feature = "imap")]
    struct ScriptedRemote {
        /// The new handle space: `(handle, link id)` pairs.
        spine: Vec<(String, String)>,
    }

    #[cfg(feature = "imap")]
    impl ReplicaRemote for ScriptedRemote {
        type Error = anyhow::Error;

        fn enumerate(
            &mut self,
            _collection: &ReplicaCollectionId,
            _cursor: Option<ReplicaCheckpoint>,
        ) -> Result<ReplicaRemoteSnapshot, Self::Error> {
            Ok(ReplicaRemoteSnapshot {
                items: self
                    .spine
                    .iter()
                    .map(|(handle, _)| ReplicaRemoteItem {
                        handle: ReplicaHandle(handle.clone()),
                        flags: ReplicaFlags::default(),
                        revision: None,
                    })
                    .collect(),
                vanished: Vec::new(),
                complete: true,
                checkpoint: ReplicaCheckpoint(crate::imap::backend::encode_checkpoint(2, 1)),
            })
        }

        fn fetch(
            &mut self,
            _collection: &ReplicaCollectionId,
            handles: Vec<ReplicaHandle>,
            _tier: ReplicaTier,
        ) -> Result<Vec<ReplicaFetchedItem>, Self::Error> {
            Ok(handles
                .into_iter()
                .filter_map(|handle| {
                    let link = self
                        .spine
                        .iter()
                        .find(|(h, _)| *h == handle.0)
                        .map(|(_, link)| link.clone())?;
                    Some(ReplicaFetchedItem {
                        handle,
                        link_id: ReplicaLinkId(link),
                        meta: ReplicaMeta(r#"{"v":1,"subject":"s"}"#.into()),
                        sort_key: ReplicaSortKey::default(),
                        body: None,
                        revision: None,
                    })
                })
                .collect())
        }

        fn push(
            &mut self,
            _collection: &ReplicaCollectionId,
            _changes: Vec<ReplicaChange>,
        ) -> Result<Vec<ReplicaPushResult>, Self::Error> {
            anyhow::bail!("scripted remote rejects pushes")
        }
    }

    #[test]
    #[cfg(feature = "imap")]
    fn a_rekey_carries_state_by_link_id_and_bumps_the_generation_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path(), "left").unwrap();
        store.ensure_collection("INBOX", "message/rfc822").unwrap();

        // The old handle space: UID 1 under UIDVALIDITY 1, hydrated.
        store
            .write(vec![
                ReplicaWriteOp::StoreObject {
                    object: ReplicaObject {
                        hash: ReplicaHash("beef0001".into()),
                        size: 3,
                    },
                    body: Some(b"abc".to_vec()),
                },
                ReplicaWriteOp::UpsertPlacement(ReplicaPlacement {
                    collection: ReplicaCollectionId("INBOX".into()),
                    handle: ReplicaHandle("1".into()),
                    link_id: Some(ReplicaLinkId("mid:a@x".into())),
                    object: Some(ReplicaHash("beef0001".into())),
                    level: ReplicaLevel::Full,
                    meta: Some(ReplicaMeta(r#"{"v":1,"subject":"s"}"#.into())),
                    sort_key: ReplicaSortKey::default(),
                    flags: ReplicaFlags::default(),
                    status: ReplicaStatus::Clean,
                    conflict_revision: None,
                    base: Some(ReplicaBase {
                        flags: ReplicaFlags::default(),
                        revision: None,
                        object: Some(ReplicaHash("beef0001".into())),
                    }),
                    origin: None,
                }),
                ReplicaWriteOp::SetCheckpoint {
                    collection: ReplicaCollectionId("INBOX".into()),
                    checkpoint: ReplicaCheckpoint(crate::imap::backend::encode_checkpoint(1, 1)),
                },
            ])
            .unwrap();
        assert_eq!(store.generation("INBOX").unwrap(), Some(1));

        // The server renumbered: the same message now lives under UID 7.
        let mut remote = ScriptedRemote {
            spine: vec![(String::from("7"), String::from("mid:a@x"))],
        };
        let (rekey, generation) = drive_rekey(&mut store, &mut remote, "INBOX").unwrap();

        // Carried by link id (the body survives without a refetch), and the
        // generation bumped exactly once, atomically with the rebuild.
        assert_eq!(rekey.rekeyed, 1);
        assert_eq!(rekey.pulled, 0);
        assert_eq!(generation, 2);
        assert_eq!(store.generation("INBOX").unwrap(), Some(2));

        let placements = load_side(&store, "INBOX").unwrap();
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].handle.0, "7");
        assert_eq!(placements[0].object, Some(ReplicaHash("beef0001".into())));
        assert_eq!(placements[0].status, ReplicaStatus::Clean);

        // The stored checkpoint now carries the new UIDVALIDITY: the guard
        // reads pre 1 / post 2 as the change that triggers this rebuild.
        assert_eq!(stored_checkpoint_uid_validity(&store, "INBOX"), Some(2));
    }

    #[test]
    fn no_collection_name_is_reserved() {
        // The Outbox used to be filtered out of every listing, whatever the
        // filter said. Submission is a queue intent now, so a remote folder
        // of that name syncs like any other one.
        let collections: HashSet<String> = ["INBOX", "Outbox", "Sent"].map(String::from).into();

        let filtered = filter_mailboxes(&collections, &CollectionFilter::All);
        assert_eq!(filtered.len(), 3);
        assert!(filtered.contains("Outbox"));

        let filtered = filter_mailboxes(
            &collections,
            &CollectionFilter::Include(vec![String::from("Outbox")]),
        );
        assert_eq!(filtered, BTreeSet::from([String::from("Outbox")]));
    }

    #[test]
    fn a_submit_intent_survives_the_drain_and_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path(), "left").unwrap();
        store.ensure_collection("Sent", "message/rfc822").unwrap();

        // A producer stages the body durably, then enqueues the intent
        // anchored on whatever collection it chose.
        let blobs = store.blobs();
        let mut writer = blobs.writer().unwrap();
        std::io::Write::write_all(&mut writer, b"Subject: hi\r\n\r\nhello").unwrap();
        let hash = ReplicaHash("cafe0002".into());
        let size = writer.commit(&hash).unwrap();

        let mut producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
        producer
            .enqueue(
                "Sent",
                &PimdirAction::Unknown {
                    kind: submit::SUBMIT.into(),
                    payload: r#"{"v":1,"object":"cafe0002","from":"a@x.org","rcpts":["b@y.org"],"subject":"hi"}"#
                        .into(),
                    object_hash: Some(hash),
                },
                Some(size),
                "2026-08-07T00:00:00Z",
            )
            .unwrap();

        // The store's drain cannot perform an intent, so it skips the row:
        // pending, not parked, and no item was created in `Sent`.
        let mut report = SyncReport::default();
        drain_queues(&mut store, &mut report);
        assert!(report.drained.is_empty());
        assert!(report.parked.is_empty());
        assert!(load_side(&store, "Sent").unwrap().is_empty());

        // Neverest reads it back, with its envelope and its pinned body.
        let intents = submit::pending(&store).unwrap();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].collection, "Sent");
        assert_eq!(intents[0].subject().as_deref(), Some("hi"));
        assert_eq!(
            intents[0].object.as_ref().map(|h| h.0.as_str()),
            Some("cafe0002")
        );

        // Acknowledging it (what a successful send does) takes the row out
        // and releases the body's pin.
        assert!(store.drop_action(intents[0].id).unwrap());
        assert!(submit::pending(&store).unwrap().is_empty());
    }

    #[test]
    fn the_retention_sweep_runs_only_when_a_delay_is_configured() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path(), "left").unwrap();
        let mut config: AccountConfig =
            toml::from_str(r#"left.imap.server = "imaps://imap.example.org:993""#).unwrap();

        // Unset means never purge: the sweep does not run, and the report
        // carries no retention section at all.
        let mut report = SyncReport::default();
        sweep_retained(&config, &mut store, &mut report);
        assert!(report.purged.is_none());
        assert!(config.store.purge_cutoff(chrono::Utc::now()).is_none());

        // Configured, the run says what it reclaimed (nothing, here: the
        // store holds no retained item).
        config.store.purge_after = Some(crate::config::HumanDuration(
            std::time::Duration::from_secs(0),
        ));
        sweep_retained(&config, &mut store, &mut report);
        let purged = report.purged.expect("a retention section");
        assert_eq!(purged.items, 0);
        assert_eq!(purged.bytes, 0);
    }

    /// A mutable-content remote: every item carries an ETag, and a write is
    /// conditional on it.
    ///
    /// This is the whole point of phase 3 — the path no compiled-in backend
    /// exercises yet, proven here before any DAV code exists, so a CardDAV
    /// failure later is a protocol bug rather than an engine one.
    struct MutableRemote {
        /// `handle -> (revision, accepted body)`.
        items: HashMap<String, (String, Option<ReplicaHash>)>,
        /// The revision handed out by the next accepted write.
        next_revision: String,
        /// Every change this remote was asked to push, in order.
        pushed: Vec<ReplicaChange>,
    }

    impl MutableRemote {
        fn at(handle: &str, revision: &str) -> Self {
            Self {
                items: HashMap::from([(handle.to_string(), (revision.to_string(), None))]),
                next_revision: String::from("v2"),
                pushed: Vec::new(),
            }
        }
    }

    impl ReplicaRemote for MutableRemote {
        type Error = anyhow::Error;

        fn enumerate(
            &mut self,
            _collection: &ReplicaCollectionId,
            _cursor: Option<ReplicaCheckpoint>,
        ) -> Result<ReplicaRemoteSnapshot, Self::Error> {
            Ok(ReplicaRemoteSnapshot {
                items: self
                    .items
                    .iter()
                    .map(|(handle, (revision, _))| ReplicaRemoteItem {
                        handle: ReplicaHandle(handle.clone()),
                        // A backend with no flag concept reports known-empty,
                        // never unknown.
                        flags: ReplicaFlags::default(),
                        revision: Some(revision.clone()),
                    })
                    .collect(),
                vanished: Vec::new(),
                complete: true,
                checkpoint: ReplicaCheckpoint(b"token-1".to_vec()),
            })
        }

        fn fetch(
            &mut self,
            _collection: &ReplicaCollectionId,
            handles: Vec<ReplicaHandle>,
            _tier: ReplicaTier,
        ) -> Result<Vec<ReplicaFetchedItem>, Self::Error> {
            Ok(handles
                .into_iter()
                .filter_map(|handle| {
                    let (revision, _) = self.items.get(&handle.0)?;
                    Some(ReplicaFetchedItem {
                        handle,
                        link_id: ReplicaLinkId("uid:a".into()),
                        meta: ReplicaMeta(r#"{"v":1}"#.into()),
                        sort_key: ReplicaSortKey::default(),
                        body: None,
                        revision: Some(revision.clone()),
                    })
                })
                .collect())
        }

        fn push(
            &mut self,
            _collection: &ReplicaCollectionId,
            changes: Vec<ReplicaChange>,
        ) -> Result<Vec<ReplicaPushResult>, Self::Error> {
            let mut results = Vec::new();
            for change in changes {
                self.pushed.push(change.clone());
                let result = match &change {
                    ReplicaChange::Update {
                        handle,
                        object,
                        if_match,
                    } => {
                        let current = self.items.get(&handle.0).map(|(rev, _)| rev.clone());
                        // The whole contract: write only if the caller's base
                        // still matches, else reject so the engine re-merges.
                        if if_match.is_some() && if_match.as_deref() == current.as_deref() {
                            let revision = self.next_revision.clone();
                            self.items
                                .insert(handle.0.clone(), (revision.clone(), Some(object.clone())));
                            ReplicaPushResult {
                                handle: handle.clone(),
                                outcome: ReplicaPushOutcome::Accepted,
                                assigned: None,
                                revision: Some(revision),
                            }
                        } else {
                            ReplicaPushResult {
                                handle: handle.clone(),
                                outcome: ReplicaPushOutcome::Rejected,
                                assigned: None,
                                revision: None,
                            }
                        }
                    }
                    other => anyhow::bail!("unexpected push: {other:?}"),
                };
                results.push(result);
            }
            Ok(results)
        }
    }

    /// Seeds a store holding one locally edited item: its body points at
    /// `edited`, its base at `original` and revision `base_revision`, i.e. the
    /// state a frontend leaves behind after staging an edit offline.
    fn store_with_local_edit(dir: &std::path::Path, base_revision: &str) -> PimdirStore {
        let mut store = PimdirStore::open(dir, "left").unwrap();
        store.ensure_collection("contacts", "text/vcard").unwrap();
        store
            .write(vec![
                ReplicaWriteOp::StoreObject {
                    object: ReplicaObject {
                        hash: ReplicaHash("0rig".into()),
                        size: 3,
                    },
                    body: Some(b"old".to_vec()),
                },
                ReplicaWriteOp::StoreObject {
                    object: ReplicaObject {
                        hash: ReplicaHash("ed17".into()),
                        size: 3,
                    },
                    body: Some(b"new".to_vec()),
                },
                ReplicaWriteOp::UpsertPlacement(ReplicaPlacement {
                    collection: ReplicaCollectionId("contacts".into()),
                    handle: ReplicaHandle("card1".into()),
                    link_id: Some(ReplicaLinkId("uid:a".into())),
                    object: Some(ReplicaHash("ed17".into())),
                    level: ReplicaLevel::Full,
                    meta: Some(ReplicaMeta(r#"{"v":1}"#.into())),
                    sort_key: ReplicaSortKey::default(),
                    flags: ReplicaFlags::default(),
                    status: ReplicaStatus::Dirty,
                    conflict_revision: None,
                    base: Some(ReplicaBase {
                        flags: ReplicaFlags::default(),
                        revision: Some(base_revision.to_string()),
                        object: Some(ReplicaHash("0rig".into())),
                    }),
                    origin: None,
                }),
            ])
            .unwrap();
        store
    }

    fn sync_with(
        store: &mut PimdirStore,
        remote: &mut MutableRemote,
        rights: ReplicaPushRights,
    ) -> ReplicaSyncReport {
        let opts = ReplicaSyncOptions {
            push: true,
            rights,
            ..Default::default()
        };
        drive(
            store,
            remote,
            ReplicaSync::new(String::from("contacts"), opts),
        )
        .unwrap()
    }

    #[test]
    fn a_local_body_edit_is_pushed_conditionally_and_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_with_local_edit(dir.path(), "v1");
        // The remote still holds the revision the edit was based on.
        let mut remote = MutableRemote::at("card1", "v1");

        let report = sync_with(&mut store, &mut remote, ReplicaPushRights::all());

        // Pushed as a conditional in-place update, not a delete + re-add.
        assert!(
            matches!(
                remote.pushed.as_slice(),
                [ReplicaChange::Update { if_match, .. }] if if_match.as_deref() == Some("v1")
            ),
            "expected one If-Match update, got {:?}",
            remote.pushed
        );
        assert_eq!(report.conflicts, 0);
        assert_eq!(report.rejected, 0);

        // The remote now holds the edited body under its new revision, and the
        // placement is clean and rebased onto it.
        assert_eq!(
            remote.items["card1"],
            (String::from("v2"), Some(ReplicaHash("ed17".into())))
        );
        let placements = load_side(&store, "contacts").unwrap();
        assert_eq!(placements[0].status, ReplicaStatus::Clean);
        let base = placements[0].base.as_ref().expect("a rebased base");
        assert_eq!(base.revision.as_deref(), Some("v2"));
        assert_eq!(base.object, Some(ReplicaHash("ed17".into())));
    }

    #[test]
    fn a_body_edited_on_both_sides_is_left_conflicted_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_with_local_edit(dir.path(), "v1");
        // Someone else edited the same contact since: the remote has moved on.
        let mut remote = MutableRemote::at("card1", "v9");

        let report = sync_with(&mut store, &mut remote, ReplicaPushRights::all());

        // The remote body is untouched — losing an edit is worse than stopping.
        assert_eq!(
            remote.items["card1"].1, None,
            "remote must not be clobbered"
        );
        assert_eq!(report.conflicts, 1);
        assert!(
            report
                .events
                .iter()
                .any(|e| matches!(e, ReplicaEvent::Conflicted(h) if h.0 == "card1")),
            "expected a Conflicted event, got {:?}",
            report.events
        );

        // The conflict must also survive the round trip through the store, not
        // just be reported by the run: io-replica's hub round-trips it on the
        // binding and io-pimdir persists it (`bindings.conflicted` /
        // `bindings.conflict_revision`). Before that landed the placement read
        // back `Dirty`, so the next run re-derived the same push, re-conflicted
        // and re-reported forever, and a frontend reading the store could not
        // tell which items needed a human. Inert for mail (immutable bodies
        // never conflict), fatal for CardDAV and CalDAV.
        //
        // Both fixes are in their working trees and not published yet, which is
        // why Cargo.toml patches them; this assertion is what proves the patch
        // is doing its job.
        let placements = load_side(&store, "contacts").unwrap();
        assert_eq!(
            placements[0].status,
            ReplicaStatus::Conflict,
            "the conflict must survive the round trip through the store"
        );
        assert_eq!(
            placements[0].conflict_revision.as_deref(),
            Some("v9"),
            "the conflicting remote revision must survive with it"
        );
    }

    #[test]
    fn a_side_denied_item_update_keeps_the_edit_pending() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_with_local_edit(dir.path(), "v1");
        let mut remote = MutableRemote::at("card1", "v1");

        // `item.update = false` on this side.
        let rights = ReplicaPushRights {
            content: false,
            ..ReplicaPushRights::all()
        };
        sync_with(&mut store, &mut remote, rights);

        assert!(
            remote.pushed.is_empty(),
            "a forbidden update must not reach the remote, got {:?}",
            remote.pushed
        );
        // Kept pending rather than dropped: the edit survives for a run where
        // the permission is granted.
        let placements = load_side(&store, "contacts").unwrap();
        assert_eq!(placements[0].status, ReplicaStatus::Dirty);
        assert_eq!(placements[0].object, Some(ReplicaHash("ed17".into())));
    }

    #[test]
    fn permissions_map_onto_io_replica_push_rights() {
        let perms = SidePermissions {
            collection: crate::config::CollectionSidePermissions::default(),
            flag: crate::config::FlagSidePermissions { update: false },
            item: crate::config::ItemSidePermissions {
                create: true,
                delete: false,
                update: true,
            },
        };
        let ctx_rights = ReplicaPushRights {
            flags: perms.flag.update,
            content: perms.item.update,
            add: perms.item.create,
            remove: perms.item.delete,
        };
        assert!(!ctx_rights.flags);
        assert!(ctx_rights.content);
        assert!(ctx_rights.add);
        assert!(!ctx_rights.remove);
    }
}
