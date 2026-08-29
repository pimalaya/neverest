//! Per-account and per-collection orchestration over the io-replica engine.
//!
//! Each collection's two sides are the two sources of one shared collection in
//! a pimdir store: one [`PimdirSourceStore`] handle per side over the same
//! files, the collection name as the bare collection id. The driver only runs
//! the engine's per-side sync, an upgrade to resolve link ids and a `Full`
//! upgrade to hydrate a body about to be copied. Cross-side propagation of
//! items, flags and deletions falls out of the shared hub's project and absorb,
//! so there is no hand-rolled cross-merge, and syncing the sides in turn until
//! quiescent converges them. The topology mismatch this resolves is in the
//! crate header.
//!
//! Collection deletion is not propagated yet, only creation, and the itemized
//! report lists cross-side copies, flags and deletes, the per-side server
//! reconcile being internal. A `Full` fetch opens a small connection pool to
//! stream several bodies at once, largest-first; the rest of a side runs on one
//! connection.

use std::{
    cmp::Reverse,
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    io::Write,
    mem,
    path::{Path, PathBuf},
    process,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Instant,
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use crossbeam_queue::SegQueue;
use io_pimdir::{
    PimdirBlobs, PimdirError, PimdirProducer, PimdirSourceStore, PimdirStore, codec::PimdirAction,
};
use io_replica::{
    client::{ReplicaRemote, ReplicaStorage},
    collection::ReplicaCollectionId,
    coroutine::{ReplicaArg, ReplicaCoroutine, ReplicaCoroutineState, ReplicaYield},
    object::ReplicaHash,
    placement::{
        ReplicaFlags, ReplicaHandle, ReplicaLevel, ReplicaMeta, ReplicaPlacement, ReplicaStatus,
    },
    rekey::{ReplicaRekey, ReplicaRekeyReport},
    remote::{ReplicaFetchedItem, ReplicaTier},
    storage::ReplicaLoadScope,
    sync::{
        ReplicaConflictPolicy, ReplicaDeletePolicy, ReplicaEvent, ReplicaPushRights, ReplicaSync,
        ReplicaSyncOptions, ReplicaSyncReport,
    },
    upgrade::ReplicaUpgrade,
};
use log::{debug, info, warn};
use pimalaya_cli::spinner::Spinner;

#[cfg(any(feature = "smtp", feature = "msgraph"))]
use crate::sync::report::SubmitEntry;
use crate::{
    account::{Account, SourceAccount},
    client::{Client, Pool},
    config::{AccountConfig, AccountMode, CollectionFilter, SourceConfig, SourcePermissions},
    item::flag::Flag,
    kind::{Kind, LinkId, merge::Merged},
    offline::{
        drive, pipe,
        remote::{
            BATCH_SIZE, CachedFetchRemote, FetchKey, PimRemote, RefusedCreate, hydrate_batch,
            resolve_kind, wire_name,
        },
        state::StoreState,
        storage::{hydration_targets, load_side, projection_view},
        submit,
    },
    sync::{
        hunk::{CollectionHunk, ItemHunk},
        report::{
            DrainedQueue, ItemConflict, ParkedQueueAction, PatchEntry, PurgedItems,
            RefusedDuplicate, SyncReport,
        },
    },
};

/// How many extra sync passes to run after the first, propagating and settling
/// cross-side changes. Two or three passes converge a collection in practice,
/// so the cap only guards against a pathological loop.
const MAX_EXTRA_PASSES: usize = 4;

/// The remote name behind a hub collection id, for a report the user reads.
///
/// A report names the collection the way its server does, not the way the store
/// keys it: that is the name the user typed into `--include-collection` and the
/// one they see in their client.
fn display_name<'a>(namespace: &str, collection: &'a str) -> &'a str {
    wire_name(namespace, collection)
}

/// The hub collection id a remote collection name binds to in `namespace`.
///
/// A hub collection is keyed by its kind, its namespace and its name, and this
/// is where the last two meet: a mailbox and an address book both called
/// `Default` land on different ids, and two providers cached side by side never
/// share one. The kind rides on the collection row, declared by
/// `ensure_collection`.
///
/// [`crate::offline::remote::PimRemote`] strips the prefix back off before any
/// call reaches the wire, so a server only ever sees the name it gave.
fn hub_id(namespace: &str, name: &str) -> String {
    format!("{namespace}/{name}")
}

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

/// How many connections a side may open. An HTTP backend keeps one: extra
/// connections only pay extra token acquisitions, the API being
/// request/response anyway.
fn connection_budget(config: &SourceConfig, connections: usize) -> usize {
    if config.is_imap() {
        connections.max(1)
    } else {
        1
    }
}

/// Opens one source handle over the account's store, grouping every collection
/// it writes under `account` (pimdir SPEC §9.2) so a store shared by two
/// hand-written accounts tells whose collection is whose.
///
/// A store an earlier draft of the format wrote cannot be migrated in place, so
/// the refusal is answered with the one command that fixes it: the store is a
/// derived cache, and dropping it costs a resync.
fn open_store(dir: &Path, source: &str, account: &str) -> Result<PimdirSourceStore> {
    match PimdirStore::open(dir) {
        Ok(store) => Ok(store.for_account(account).for_source(source)),
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
/// The kind is never configured, falling out of each side's backend, so the
/// configuration schema cannot express "these two sides are compatible" and
/// this check is the enforcement point. It runs after the sides open, the
/// earliest moment their media types are known, and before the first store
/// write, so a mismatched account fails with nothing half-written.
fn check_kinds(left: &mut SourceCtx, right: &mut SourceCtx) -> Result<Kind> {
    let left = (
        left.name.clone(),
        left.pool.primary().media_type().to_string(),
    );
    let right = (
        right.name.clone(),
        right.pool.primary().media_type().to_string(),
    );
    let left = (left.0.as_str(), left.1.as_str());
    let right = (right.0.as_str(), right.1.as_str());
    pair_kind(left, right)
}

/// The pure half of [`check_kinds`]: the two sides' media types in, the one
/// kind they agree on out.
fn pair_kind(left: (&str, &str), right: (&str, &str)) -> Result<Kind> {
    let resolve = |(source, raw): (&str, &str)| -> Result<Kind> {
        Kind::from_media_type(raw).with_context(|| {
            format!("This build cannot sync items of type {raw} (source {source})")
        })
    };

    let left_kind = resolve(left)?;
    let right_kind = resolve(right)?;

    if left_kind != right_kind {
        bail!(
            "The two sides sync different kinds and cannot be reconciled: \
             {} is {} and {} is {}. \
             Pair each kind with its own account (they may share a `store.root`).",
            left.0,
            left_kind.media_type(),
            right.0,
            right_kind.media_type(),
        );
    }

    Ok(left_kind)
}

/// Live per-collection progress: the collection spinner and its base label, so
/// an inner phase such as body hydration or relay can append a percentage to
/// the `[2/7] Syncing INBOX` line.
struct CollectionProgress<'a> {
    spinner: &'a Spinner,
    label: &'a str,
}

impl CollectionProgress<'_> {
    /// Updates the spinner to `<label> <percent>%` of `done`/`total`.
    fn tick(&self, done: usize, total: usize) {
        let percent = (done * 100).checked_div(total).unwrap_or(100);
        self.spinner
            .set_message(format!("{} ({percent}%)", self.label));
    }
}

/// Which side wins when an endpoint and the store disagree about one item.
///
/// This is the whole of `one-way`. A namespace could say which endpoints met
/// and never which way, so both sides were authoritative and every divergence
/// had to become a conflict; declaring an authority is what removes the
/// conflict rather than resolving it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Authority {
    /// Neither side wins: a divergence is recorded as a conflict for the user
    /// to resolve. The two-way default.
    Shared,
    /// The endpoint wins: its version replaces the store's, and the store never
    /// writes back. The `sources` side under `one-way`.
    Endpoint,
    /// The store wins: its version is pushed over the endpoint's, and what the
    /// endpoint changed on its own is overwritten. The `targets` side under
    /// `one-way`.
    Store,
}

impl Authority {
    /// How a divergence between the endpoint and the store resolves.
    ///
    /// `remote` is the endpoint and `local` is the store, so an authoritative
    /// endpoint prefers the remote and an authoritative store prefers the
    /// local. Neither records a conflict, which is the point: under `one-way`
    /// there is nothing for a user to resolve.
    fn conflict_policy(self) -> ReplicaConflictPolicy {
        match self {
            Self::Shared => ReplicaConflictPolicy::Manual,
            Self::Endpoint => ReplicaConflictPolicy::PreferRemote,
            Self::Store => ReplicaConflictPolicy::PreferLocal,
        }
    }

    /// Whether anything is ever written back to the endpoint. False for an
    /// authoritative one: what it holds is the truth.
    fn writes_back(self) -> bool {
        !matches!(self, Self::Endpoint)
    }
}

/// One connected endpoint plus its permissions, its authority, and whether it
/// may push.
struct SourceCtx {
    /// The endpoint's configured name, which is its pimdir source id and the
    /// name every hunk this run reports carries.
    name: String,
    /// The hub namespace it binds into, stripped back off a collection id
    /// before any call reaches the wire.
    namespace: String,
    pool: Pool,
    perms: SourcePermissions,
    authority: Authority,
    /// The creates this endpoint refused because it already holds the item's
    /// identity, collected by the remote each pass pushes and drained into the
    /// report by [`itemize_refused`].
    refused: Vec<RefusedCreate>,
}

impl SourceCtx {
    fn conflict_policy(&self) -> ReplicaConflictPolicy {
        self.authority.conflict_policy()
    }

    /// A side pushes unless it forbids every item and flag mutation (a fully
    /// read-only source), or unless it is authoritative, which is the same
    /// statement made by the account rather than by a permission table.
    fn writable(&self) -> bool {
        let rights = self.push_rights();
        self.authority.writes_back()
            && (rights.flags || rights.content || rights.add || rights.remove)
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

/// Runs the whole account sync and returns the report.
///
/// An account is one hub and one mode (see [`AccountConfig::mode`]). With no
/// target each source is reconciled against the store the app reads; with
/// targets each source is reconciled against every one of them, propagation
/// falling out of the shared hub. Sources never see each other: their
/// collection ids differ, which is what keeps a mail source and a contacts
/// source, or two providers cached side by side, from meeting.
///
/// What the store keeps is declared by `retain` rather than derived here, so
/// nothing about it is reported: the configuration already says it.
#[allow(clippy::too_many_arguments)]
pub fn run(
    account_name: impl Into<String>,
    account_config: &AccountConfig,
    collection_filter: Option<CollectionFilter>,
    dry_run: bool,
    connections: usize,
    no_purge: bool,
    only_sources: &[String],
    accept_mode: bool,
) -> Result<SyncReport> {
    let account_name = account_name.into();
    let endpoints = account_config.endpoints()?;
    let mode = account_config.mode()?;
    let running = select_sources(&mode, only_sources)?;

    let real_dir = store_dir(&account_name, account_config)?;
    // Held for the whole run: dropping it is what removes the replica, so
    // an early return cannot leave one behind.
    let dry_replica = dry_run.then(|| DryRunReplica::new(&real_dir)).transpose()?;
    let work_dir = match &dry_replica {
        Some(replica) => replica.dir.clone(),
        None => real_dir.clone(),
    };
    fs::create_dir_all(&work_dir)
        .with_context(|| format!("Create replica dir {} error", work_dir.display()))?;

    let mut state = StoreState::load(&work_dir)?;

    let mut report = SyncReport {
        account: account_name.clone(),
        dry_run,
        ..Default::default()
    };

    // Refuses a mode change that would discard what the previous one kept,
    // before any endpoint is opened.
    state.check_mode(&mode)?;

    // Every credential the run needs, read once here rather than once per
    // opened connection.
    let account = Account::resolve(account_config)?;

    for source_name in &running {
        let source = endpoints[source_name].clone();

        // A source is reconciled against the local store, then, where the
        // account names targets, what it holds crosses to each of them. Sources
        // never meet: an item held by one is pushed to the targets, never to
        // another source.
        let outcome = if mode.is_local() {
            run_local(
                &account_name,
                account_config,
                &account,
                &mode,
                source_name,
                source,
                collection_filter.clone(),
                &work_dir,
                dry_run,
                connections,
                &mut report,
            )
        } else {
            run_targets(
                &account_name,
                account_config,
                &account,
                &mode,
                &endpoints,
                source_name,
                source,
                collection_filter.clone(),
                &work_dir,
                dry_run,
                connections,
                &mut report,
            )
        };

        // A source that fails is reported and the next one still runs: they
        // share nothing but the file the store lives in, so one broken remote
        // is no reason to leave the others unsynced.
        if let Err(err) = outcome {
            warn!("source {source_name} sync error: {err:#}");
            report.collection.patch.push(PatchEntry::new(
                CollectionHunk::Scan {
                    side: source_name.clone(),
                    collection: String::from("*"),
                },
                Some(err),
            ));
        }
    }

    // Once for the run, whatever the sources did and whether or not this is a
    // dry run: the parked rows and the outstanding conflicts are the store's,
    // and a source-by-source read would report each of them once per source.
    match PimdirStore::open(&work_dir) {
        Ok(store) => {
            report_parked(&store, &mut report);
            count_conflicts(&store, &account_name, &mut report);
        }
        Err(err) => warn!("cannot open the store to read what it left behind: {err}"),
    }

    announce_conflicts(account_config, &report);

    if !dry_run {
        state.record_mode(&mode, accept_mode);

        let sweeper = running
            .first()
            .expect("a validated account has at least one source");
        let mut store = open_store(&work_dir, sweeper, &account_name)?;

        if !no_purge {
            sweep_retained(account_config, &mut store, &mut report);
        }

        state.save(&work_dir)?;
    }

    crate::offline::prof::report();
    Ok(report)
}

/// The sources this run touches: every one of them, or those `--source` named.
///
/// Narrowing picks sources rather than namespaces, there being none: an account
/// is one mode, and a source is reconciled on its own against the store and
/// whatever targets the account declares.
fn select_sources(mode: &AccountMode, only: &[String]) -> Result<Vec<String>> {
    if only.is_empty() {
        return Ok(mode.sources.clone());
    }

    if let Some(unknown) = only.iter().find(|name| !mode.sources.contains(name)) {
        bail!(
            "This account has no source named {unknown}. It declares {}.",
            mode.sources.join(", "),
        );
    }

    Ok(mode
        .sources
        .iter()
        .filter(|name| only.contains(name))
        .cloned()
        .collect())
}

/// One source against every target the account names, each pairing run on its
/// own.
///
/// Both endpoints bind the source's namespace, which is what makes them meet:
/// an item the source holds with no binding for the target is pushed to it.
/// Targets are run in turn rather than together, because under `one-way` the
/// source is authoritative and no target can influence another, so a pairing
/// is complete on its own. The source is therefore enumerated once per target,
/// which the single-target migration this exists for never notices.
#[allow(clippy::too_many_arguments)]
fn run_targets(
    account_name: &str,
    account_config: &AccountConfig,
    account: &Account,
    mode: &AccountMode,
    endpoints: &HashMap<String, SourceConfig>,
    source_name: &str,
    source_config: SourceConfig,
    collection_filter: Option<CollectionFilter>,
    work_dir: &Path,
    dry_run: bool,
    connections: usize,
    report: &mut SyncReport,
) -> Result<()> {
    let relay = mode.streams(endpoints);

    for target_name in &mode.targets {
        run_pair(
            account_name,
            account_config,
            account,
            mode,
            source_name,
            source_name.to_string(),
            source_config.clone(),
            target_name.clone(),
            endpoints[target_name].clone(),
            collection_filter.clone(),
            relay,
            work_dir,
            dry_run,
            connections,
            report,
        )?;
    }

    Ok(())
}

/// One source against one target over the shared hub, cross-endpoint
/// propagation falling out of it.
#[allow(clippy::too_many_arguments)]
fn run_pair(
    account_name: &str,
    account_config: &AccountConfig,
    account: &Account,
    mode: &AccountMode,
    namespace: &str,
    left_name: String,
    left_config: SourceConfig,
    right_name: String,
    right_config: SourceConfig,
    collection_filter: Option<CollectionFilter>,
    relay: bool,
    work_dir: &Path,
    dry_run: bool,
    connections: usize,
    report: &mut SyncReport,
) -> Result<()> {
    let left_filter = left_config.collection().filter.clone();

    let mut left_store = open_store(work_dir, &left_name, account_name)?;
    let mut right_store = open_store(work_dir, &right_name, account_name)?;
    let blobs = left_store.blobs();

    drain_queues(&mut left_store, namespace, report);

    // Declared, not derived: `retain` says whether the store is a replica, and
    // `relay` is only how a crossing gets there when it is not.
    let hydrate_full = mode.retain;

    let left_budget = connection_budget(&left_config, connections);
    let right_budget = connection_budget(&right_config, connections);

    // Under `one-way` the source is the truth and the target follows it, so
    // neither side records a conflict: the store takes the source's version and
    // pushes it over the target's. Without it both sides are shared and a
    // divergence is a conflict, which is the two-way mirror.
    let (left_authority, right_authority) = if mode.one_way {
        (Authority::Endpoint, Authority::Store)
    } else {
        (Authority::Shared, Authority::Shared)
    };

    let left_account = account.get(&left_name)?;
    let right_account = account.get(&right_name)?;

    let s = Spinner::start("Opening endpoints…");
    let mut left = SourceCtx {
        name: left_name.clone(),
        namespace: namespace.to_string(),
        perms: left_config.permissions(),
        authority: left_authority,
        pool: Pool::open(left_account, left_budget)
            .with_context(|| format!("Open source {left_name}"))?,
        refused: Vec::new(),
    };
    let mut right = SourceCtx {
        name: right_name.clone(),
        namespace: namespace.to_string(),
        perms: right_config.permissions(),
        authority: right_authority,
        pool: Pool::open(right_account, right_budget)
            .with_context(|| format!("Open target {right_name}"))?,
        refused: Vec::new(),
    };
    s.success("Opened endpoints");

    let kind = check_kinds(&mut left, &mut right)?;
    let media_type = kind.media_type();

    if !dry_run {
        drain_submits(
            account_config,
            account,
            &mut [&mut left, &mut right],
            &mut left_store,
            &blobs,
            report,
        );
    }

    let s = Spinner::start("Listing collections…");
    let left_collections = list_collections(left.pool.primary())?;
    let right_collections = list_collections(right.pool.primary())?;
    s.success(format!(
        "Listed collections ({} on {left_name}, {} on {right_name})",
        left_collections.len(),
        right_collections.len()
    ));

    let filter = collection_filter.unwrap_or(left_filter);
    let left_filtered = filter_collections(&left_collections, &filter);
    let right_filtered = filter_collections(&right_collections, &filter);

    let collection_hunks = diff_collections(&left_filtered, &right_filtered, &left, &right);
    for hunk in collection_hunks {
        let error = if dry_run {
            None
        } else {
            apply_collection_hunk(&hunk, &mut left, &mut right).err()
        };
        report.collection.patch.push(PatchEntry::new(hunk, error));
    }

    let mut present_left = left_filtered.clone();
    let mut present_right = right_filtered.clone();
    for entry in &report.collection.patch {
        if entry.error.is_some() {
            continue;
        }
        if let CollectionHunk::Create { side, collection } = &entry.hunk {
            if *side == left_name {
                present_left.insert(collection.clone());
            } else if *side == right_name {
                present_right.insert(collection.clone());
            }
        }
    }
    let common: BTreeSet<String> = present_left.intersection(&present_right).cloned().collect();

    let total = common.len();
    for (index, name) in common.iter().enumerate() {
        let collection = &hub_id(namespace, name);
        let label = format!("[{}/{total}] Syncing {name}", index + 1);
        let s = Spinner::start(label.clone());
        let progress = CollectionProgress {
            spinner: &s,
            label: &label,
        };

        if let Err(err) = sync_collection(
            collection,
            media_type,
            &mut left,
            &mut right,
            &mut left_store,
            &mut right_store,
            &blobs,
            work_dir,
            dry_run,
            relay,
            &progress,
            report,
        ) {
            warn!("{name} sync error: {err:#}");
            s.success(format!("{name}: error ({err:#})"));
            continue;
        }

        if hydrate_full
            && !dry_run
            && let Err(err) = hydrate_full_collection(
                collection,
                &mut left,
                &mut right,
                &mut left_store,
                &mut right_store,
                &blobs,
                &progress,
            )
        {
            warn!("{name} full hydration error: {err:#}");
        }

        s.success(format!("{name}: done"));
    }

    Ok(())
}

/// A collection's spine result: its name and the bodies to hydrate (handle + size),
/// carried from Phase 1 (spine) into Phase 2 (hydrate) and Phase 3 (apply).
type CollectionPlan = (String, Vec<(ReplicaHandle, u64)>);

/// The local, one-source sync, run as three account-wide phases so the
/// connection pool stays saturated end to end rather than idling at collection
/// boundaries. Phase 1 spines every collection in parallel over its own
/// connection and store handle, collecting the bodies to hydrate. Phase 2
/// streams every body across all collections through one global work-stealing
/// pool into the blob store. Phase 3 applies the fetched bodies to the index
/// per collection from cache, with no network. The store is the single local
/// copy the app reads, so there is no cross-side hydration.
#[allow(clippy::too_many_arguments)]
fn run_local(
    account_name: &str,
    account_config: &AccountConfig,
    account: &Account,
    mode: &AccountMode,
    source_name: &str,
    source_config: SourceConfig,
    collection_filter: Option<CollectionFilter>,
    work_dir: &Path,
    dry_run: bool,
    connections: usize,
    report: &mut SyncReport,
) -> Result<()> {
    let source = source_name.to_string();
    let source_filter = source_config.collection().filter.clone();

    let workers = connection_budget(&source_config, connections);
    let s = Spinner::start(format!("Opening connections to {source_name}…"));
    // With no target the store is the destination: `one-way` makes the source
    // the truth and discards what was staged locally, leaving it off merges the
    // two, which is the offline replica an app writes into.
    let authority = if mode.one_way {
        Authority::Endpoint
    } else {
        Authority::Shared
    };

    let source_account = account.get(source_name)?;
    let mut ctxs = open_source_contexts(
        &source,
        source_name,
        &source_config,
        source_account,
        authority,
        workers,
    )?;
    let mut stores: Vec<PimdirSourceStore> = (0..workers)
        .map(|_| open_store(work_dir, &source, account_name))
        .collect::<Result<_>>()?;
    let blobs = stores[0].blobs();
    s.success(format!(
        "Opened {} connection(s) to {source_name}",
        ctxs.len()
    ));

    drain_queues(&mut stores[0], source_name, report);

    let raw = ctxs[0].pool.primary().media_type();
    let kind = Kind::from_media_type(raw)
        .with_context(|| format!("This build cannot sync items of type {raw}"))?;
    let media_type = kind.media_type();

    if !dry_run {
        let (first, _) = ctxs.split_first_mut().expect("at least one connection");
        drain_submits(
            account_config,
            account,
            &mut [first],
            &mut stores[0],
            &blobs,
            report,
        );
    }

    let s = Spinner::start(format!("Listing collections on {source_name}…"));
    let collections = list_collections(ctxs[0].pool.primary())?;
    s.success(format!(
        "Listed {} collection(s) on {source_name}",
        collections.len()
    ));

    let filter = collection_filter.unwrap_or(source_filter);
    let filtered: Vec<String> = filter_collections(&collections, &filter)
        .into_iter()
        .map(|name| hub_id(source_name, &name))
        .collect();

    for collection in &filtered {
        stores[0]
            .ensure_collection(collection, media_type)
            .with_context(|| format!("Declare kind for {collection}"))?;
    }

    let plans = phase1_spine(
        source_name,
        &filtered,
        &mut ctxs,
        &mut stores,
        &blobs,
        work_dir,
        dry_run,
        report,
    )?;

    if dry_run {
        return Ok(());
    }

    let cache = phase2_hydrate(source_name, &plans, &mut ctxs, &blobs)?;
    phase3_apply(
        source_name,
        &plans,
        &mut ctxs[0],
        &mut stores[0],
        &blobs,
        &cache,
    )?;

    Ok(())
}

/// Opens `count` independent single-connection [`SourceCtx`]s in parallel, so the
/// connection handshakes overlap instead of paying them one after another.
fn open_source_contexts(
    name: &str,
    namespace: &str,
    config: &SourceConfig,
    account: SourceAccount,
    authority: Authority,
    count: usize,
) -> Result<Vec<SourceCtx>> {
    let perms = config.permissions();
    let opened: Vec<Result<Pool>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..count)
            .map(|_| {
                let account = account.clone();
                scope.spawn(move || Pool::open(account, 1))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("connection open thread"))
            .collect()
    });
    let mut ctxs = Vec::with_capacity(count);
    for pool in opened {
        ctxs.push(SourceCtx {
            name: name.to_string(),
            namespace: namespace.to_string(),
            perms,
            authority,
            pool: pool.context("Open connection")?,
            refused: Vec::new(),
        });
    }
    Ok(ctxs)
}

/// Phase 1, spining each collection in parallel: a work-stealing pool of
/// workers, each on its own connection and store handle, pulls the collection
/// queue and reconciles, collecting the bodies to hydrate. Reads run
/// concurrently through WAL and writes serialise on the store's single-writer
/// lock. Report patches are merged after the barrier. A collection that errors
/// is logged and skipped, its bodies never entering the plan.
#[allow(clippy::too_many_arguments)]
fn phase1_spine(
    source: &str,
    filtered: &[String],
    ctxs: &mut [SourceCtx],
    stores: &mut [PimdirSourceStore],
    blobs: &PimdirBlobs,
    store_dir: &Path,
    dry_run: bool,
    report: &mut SyncReport,
) -> Result<Vec<CollectionPlan>> {
    let total = filtered.len();
    let queue: SegQueue<String> = SegQueue::new();
    for collection in filtered {
        queue.push(collection.clone());
    }
    let plans: Mutex<Vec<CollectionPlan>> = Mutex::new(Vec::new());
    let merged: Mutex<SyncReport> = Mutex::new(SyncReport::default());
    let scanned = AtomicUsize::new(0);
    let s = Spinner::start(format!("Scanning {source} (0/{total})"));

    let queue_ref = &queue;
    let plans_ref = &plans;
    let merged_ref = &merged;
    let scanned_ref = &scanned;
    let s_ref = &s;

    thread::scope(|scope| {
        for (ctx, store) in ctxs.iter_mut().zip(stores.iter_mut()) {
            scope.spawn(move || {
                while let Some(collection) = queue_ref.pop() {
                    match collection_spine(&collection, ctx, store, blobs, store_dir, dry_run) {
                        Ok((targets, rep)) => {
                            plans_ref.lock().unwrap().push((collection, targets));
                            merged_ref.lock().unwrap().item.patch.extend(rep.item.patch);
                        }
                        Err(err) => {
                            warn!("{collection} scan error: {err:#}");
                            let display = display_name(&ctx.namespace, &collection).to_string();
                            merged_ref
                                .lock()
                                .unwrap()
                                .collection
                                .patch
                                .push(PatchEntry::new(
                                    CollectionHunk::Scan {
                                        side: ctx.name.clone(),
                                        collection: display,
                                    },
                                    Some(err),
                                ));
                        }
                    }
                    let n = scanned_ref.fetch_add(1, Ordering::Relaxed) + 1;
                    s_ref.set_message(format!("Scanning {source} ({n}/{total})"));
                }
            });
        }
    });

    let plans = plans.into_inner().unwrap();
    let merged = merged.into_inner().unwrap();
    report.item.patch.extend(merged.item.patch);
    report.collection.patch.extend(merged.collection.patch);
    s.success(format!("Scanned {total} collection(s) on {source}"));
    Ok(plans)
}

/// Reconciles one collection's spine, without hydration: pull the remote into
/// the hub, itemize the pending local edits and the pull plan, then push and
/// settle. Returns the not-yet-`Full` bodies to hydrate, each with the size its
/// local envelope meta carries so the download can run largest-first, plus the
/// report patches. A dry run stops after itemizing, leaving the targets empty.
fn collection_spine(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    store_dir: &Path,
    dry_run: bool,
) -> Result<(Vec<(ReplicaHandle, u64)>, SyncReport)> {
    let mut report = SyncReport::default();

    let before = flag_snapshot(store, collection, &ctx.name)?;

    let pull = sync_side_rebuilding(collection, ctx, store, blobs, false)?;
    let display = display_name(&ctx.namespace, collection);
    // NOTE: before the report reads which conflicts survived. A divergence the
    // merge settles was never a disagreement, and reporting one the run
    // resolved in the same breath is the noise this whole pass removes.
    resolve_conflicts(collection, ctx, store, blobs, store_dir, dry_run)?;
    itemize_pulled(
        &pull.events,
        &before,
        store,
        collection,
        display,
        &ctx.name,
        &mut report,
    )?;
    // NOTE: before the probe, which resolves link ids and, for a kind that
    // carries no cheap `Meta` tier, does so by downloading the whole body. Read
    // after it, a card is already hydrated and reports nothing, so a run that
    // pulled a whole address book called itself quiescent.
    itemize_fetches(collection, display, store, &ctx.name, &mut report)?;
    upgrade_probed(collection, ctx, store, blobs, dry_run)?;
    itemize_single(collection, store, ctx, &mut report)?;
    if dry_run {
        return Ok((Vec::new(), report));
    }

    for _ in 0..=MAX_EXTRA_PASSES {
        let pass = sync_side_rebuilding(collection, ctx, store, blobs, ctx.writable())?;
        upgrade_probed(collection, ctx, store, blobs, false)?;
        if !moved(&pass) {
            break;
        }
    }
    itemize_refused(&ctx.name, mem::take(&mut ctx.refused), &mut report);

    let mut targets: Vec<(ReplicaHandle, u64)> = Vec::new();
    for placement in projection_view(store, collection, &ctx.name)
        .with_context(|| format!("Project {} {collection}", &ctx.name))?
    {
        if placement.status == ReplicaStatus::Tombstone || placement.object.is_some() {
            continue;
        }
        let size = meta_size(&placement.meta).unwrap_or(0) as u64;
        targets.push((placement.handle, size));
    }
    Ok((targets, report))
}

/// Phase 2, hydrating every body across every collection through one global
/// work-stealing pool. Bodies are chunked into largest-first per-collection
/// batches, since a batched fetch stays within one selected collection, and the
/// biggest batches are queued first for a global largest-first order. A worker
/// finishing one collection's last batch immediately steals the next
/// collection's, so no connection idles at a collection edge. Bodies stream
/// into the blob store, and the fetched items are cached by collection and
/// handle for Phase 3.
fn phase2_hydrate(
    source: &str,
    plans: &[CollectionPlan],
    ctxs: &mut [SourceCtx],
    blobs: &PimdirBlobs,
) -> Result<HashMap<FetchKey, ReplicaFetchedItem>> {
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

    let kind = ctxs
        .first_mut()
        .map(|ctx| resolve_kind(&mut ctx.pool))
        .unwrap_or(Kind::Mail);

    let namespace = ctxs
        .first()
        .map(|ctx| ctx.namespace.clone())
        .unwrap_or_default();

    let cache: Mutex<HashMap<FetchKey, ReplicaFetchedItem>> =
        Mutex::new(HashMap::with_capacity(total_bodies));
    let failure: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let stop = AtomicBool::new(false);
    let done = AtomicUsize::new(0);
    let s = Spinner::start(format!("Downloading {source} 0% (0/{total_bodies})"));

    let queue_ref = &queue;
    let cache_ref = &cache;
    let failure_ref = &failure;
    let stop_ref = &stop;
    let done_ref = &done;
    let s_ref = &s;
    let namespace_ref = namespace.as_str();

    thread::scope(|scope| {
        for ctx in ctxs.iter_mut() {
            scope.spawn(move || {
                let on_body = || {
                    let n = done_ref.fetch_add(1, Ordering::Relaxed) + 1;
                    let percent = (n * 100).checked_div(total_bodies).unwrap_or(100);
                    s_ref.set_message(format!(
                        "Downloading {source} {percent}% ({n}/{total_bodies})"
                    ));
                };
                while !stop_ref.load(Ordering::Relaxed) {
                    let Some((collection, handles)) = queue_ref.pop() else {
                        break;
                    };
                    match hydrate_batch(
                        kind,
                        ctx.pool.primary(),
                        wire_name(namespace_ref, &collection),
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
    s.success(format!("Downloaded {} item(s) from {source}", cache.len()));
    Ok(cache)
}

/// Phase 3, applying the pre-fetched bodies to the index one collection at a
/// time with no network: io-replica's `Full` upgrade runs over a
/// [`CachedFetchRemote`] serving each body from the Phase 2 cache, a miss
/// falling back to a real fetch. Only store writes happen here.
fn phase3_apply(
    source: &str,
    plans: &[CollectionPlan],
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    cache: &HashMap<FetchKey, ReplicaFetchedItem>,
) -> Result<()> {
    let total = plans.len();
    let s = Spinner::start(format!("Writing {source} (0/{total})"));
    for (index, (collection, _)) in plans.iter().enumerate() {
        if let Err(err) = apply_full(collection, ctx, store, blobs, cache) {
            warn!("{collection} write error: {err:#}");
        }
        s.set_message(format!("Writing {source} ({}/{total})", index + 1));
    }
    s.success(format!("Wrote {total} collection(s) on {source}"));
    Ok(())
}

/// Raises every not-yet-`Full` item of `collection` to `Full` from the pre-fetch
/// cache (blobs already on disk), so the retained store holds each body for the
/// app to read offline.
fn apply_full(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    cache: &HashMap<FetchKey, ReplicaFetchedItem>,
) -> Result<()> {
    let handles: Vec<ReplicaHandle> = projection_view(store, collection, &ctx.name)
        .with_context(|| format!("Project {} {collection}", &ctx.name))?
        .into_iter()
        .filter(|p| p.status != ReplicaStatus::Tombstone && p.level < ReplicaLevel::Full)
        .map(|p| p.handle)
        .collect();
    if handles.is_empty() {
        return Ok(());
    }
    let fallback = PimRemote::new(&mut ctx.pool, blobs.clone(), ctx.namespace.clone());
    let mut remote = CachedFetchRemote::new(cache, fallback);
    drive(
        store,
        &mut remote,
        ReplicaUpgrade::new(collection.to_string(), handles, ReplicaTier::Full),
    )
    .with_context(|| format!("Apply bodies {collection}"))?;
    Ok(())
}

/// A `handle → flags` snapshot of a side's items, taken before a pull so its
/// remote flag changes can be diffed out of the sync's per-item events.
fn flag_snapshot(
    store: &PimdirSourceStore,
    collection: &str,
    source: &str,
) -> Result<HashMap<String, ReplicaFlags>> {
    Ok(load_side(store, collection)
        .with_context(|| format!("Load {source} {collection}"))?
        .into_iter()
        .map(|placement| (placement.handle.0, placement.flags))
        .collect())
}

/// Itemizes into the report the remote-originated changes a pull applied: flag
/// changes and removals on already-synced items. The pull applies them silently,
/// the item reading `Clean` afterwards, so they are recovered from the sync's
/// per-item events. A `FlagsChanged` is diffed against the pre-pull snapshot
/// into add and remove hunks, and a `Vanished` becomes a delete. A new remote
/// item is an `Added` event but the pull plan already reports it as a `Fetch`.
///
/// A `Conflicted` event says a placement entered conflict, not that it is
/// still in one: [`resolve_conflicts`] has run since and merged away whatever
/// nobody disagreed about. So the store is asked which of them survived, and
/// only those reach the report, a divergence settled in the same run being
/// nothing a person has to hear about.
fn itemize_pulled(
    events: &[ReplicaEvent],
    before: &HashMap<String, ReplicaFlags>,
    store: &PimdirSourceStore,
    collection: &str,
    display: &str,
    source: &str,
    report: &mut SyncReport,
) -> Result<()> {
    let after = flag_snapshot(store, collection, source)?;
    let parked: HashSet<String> = match events
        .iter()
        .any(|event| matches!(event, ReplicaEvent::Conflicted(_)))
    {
        false => HashSet::new(),
        true => conflicted_placements(store, collection, source)?
            .into_iter()
            .map(|placement| placement.handle.0)
            .collect(),
    };

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
                            side: source.to_string(),
                            collection: display.to_string(),
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
                            side: source.to_string(),
                            collection: display.to_string(),
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
                        side: source.to_string(),
                        collection: display.to_string(),
                        id: handle.0.clone(),
                        content_key: 0,
                    },
                    None,
                ));
            }
            ReplicaEvent::Conflicted(handle) => {
                if !parked.contains(&handle.0) {
                    continue;
                }
                report.conflicts.push(ItemConflict {
                    side: source.to_string(),
                    collection: display.to_string(),
                    id: handle.0.clone(),
                });
            }
            _ => {}
        }
    }
    Ok(())
}

/// Itemizes one side's pending projection into the report, the outbound work a
/// flag, move, delete or compose staged, reusing the shared placement mapping.
fn itemize_single(
    collection: &str,
    store: &PimdirStore,
    ctx: &SourceCtx,
    report: &mut SyncReport,
) -> Result<()> {
    let display = display_name(&ctx.namespace, collection);
    let view = projection_view(store, collection, &ctx.name)
        .with_context(|| format!("Project {} {display}", ctx.name))?;
    for placement in view {
        for hunk in placement_hunks(&ctx.name, &ctx.name, display, &placement) {
            report.item.patch.push(PatchEntry::new(hunk, None));
        }
    }
    Ok(())
}

/// Reports the pull plan of a one-source local sync: each not-yet-`Full`,
/// non-tombstone item is a body this run fetches, named the same way whether or
/// not the run is a dry one.
///
/// Runs before the probe rather than after it. A kind with no cheap `Meta` tier
/// resolves its link id from the body, so the probe hydrates it, and a plan read
/// afterwards would be empty for exactly the items the run is about to pull.
fn itemize_fetches(
    collection: &str,
    display: &str,
    store: &PimdirSourceStore,
    source: &str,
    report: &mut SyncReport,
) -> Result<()> {
    let view =
        load_side(store, collection).with_context(|| format!("Load {source} {collection}"))?;
    for placement in view {
        if placement.status == ReplicaStatus::Tombstone || placement.object.is_some() {
            continue;
        }
        let id = placement
            .link_id
            .as_ref()
            .map(|l| l.0.clone())
            .unwrap_or_else(|| placement.handle.0.clone());
        report.item.patch.push(PatchEntry::new(
            ItemHunk::Fetch {
                side: source.to_string(),
                collection: display.to_string(),
                id,
                content_key: 0,
            },
            None,
        ));
    }
    Ok(())
}

/// Reconciles one collection: a first per-side reconcile resolving link ids and
/// pulling both servers into the hub, a hydration of the bodies about to cross,
/// then extra passes pushing the projected propagations until quiescent.
#[allow(clippy::too_many_arguments)]
fn sync_collection(
    collection: &str,
    media_type: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    store_dir: &Path,
    dry_run: bool,
    relay: bool,
    progress: &CollectionProgress,
    report: &mut SyncReport,
) -> Result<()> {
    left_store
        .ensure_collection(collection, media_type)
        .with_context(|| format!("Declare kind for {collection}"))?;

    reconcile_pass(
        collection,
        left,
        right,
        left_store,
        right_store,
        blobs,
        store_dir,
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
        report,
    )?;

    itemize(collection, left_store, left, right, report)?;
    if dry_run {
        return Ok(());
    }

    for _ in 0..MAX_EXTRA_PASSES {
        let progressed = reconcile_pass(
            collection,
            left,
            right,
            left_store,
            right_store,
            blobs,
            store_dir,
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
            report,
        )?;
        if !progressed && propagated == 0 {
            break;
        }
    }

    itemize_refused(&left.name, mem::take(&mut left.refused), report);
    itemize_refused(&right.name, mem::take(&mut right.refused), report);

    Ok(())
}

/// Propagates the collection's cross-side copies, either by retaining, which
/// hydrates the body into the store for the projection to push, or by relaying,
/// which streams the body server-to-server and keeps only the spine. Returns
/// how many bodies were moved.
#[allow(clippy::too_many_arguments)]
fn propagate(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    relay: bool,
    progress: &CollectionProgress,
    report: &mut SyncReport,
) -> Result<usize> {
    if relay {
        relay_copies(collection, left, right, left_store, progress, report)
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

/// One cross-copy body to relay: its holding side and fetch handle, the exact
/// octet length taken from the item's meta so the target append is
/// length-prefixed without buffering the body, the link id and the flags.
struct RelayTarget {
    /// The name of the source holding the body.
    holding: String,
    handle: ReplicaHandle,
    /// The cross-side identity, carried so the relayed append can be itemized
    /// under the same id the hydrating path reports a copy under.
    link: String,
    /// The identity the target addresses the new item by, from
    /// [`Kind::split_link_id`], owned because the link id it points into is
    /// dropped with the hub.
    hint: Option<String>,
    /// The minted part of the key, where the copy shares its identity with
    /// another item of the same collection, so the target names it apart from
    /// the copy already holding that identity.
    mint: Option<String>,
    size: usize,
    flags: Vec<Flag>,
}

/// Reads the relay targets from the hub: an item held by exactly one side,
/// never hydrated so with no stored object, whose far side may create it.
fn relay_targets(
    store: &PimdirStore,
    kind: Kind,
    collection: &str,
    left: (&str, bool),
    right: (&str, bool),
) -> Result<Vec<RelayTarget>> {
    let hub = store.load_hub(collection)?;
    let mut out = Vec::new();
    for (link, item) in &hub.items {
        if item.deleted || item.object.is_some() || item.sources.len() != 1 {
            continue;
        }
        let (held, binding) = item.sources.iter().next().expect("one source");
        let holding = held.0.clone();
        let target_creates = if holding == left.0 { right.1 } else { left.1 };
        if !target_creates {
            continue;
        }
        let Some(size) = meta_size(&item.meta) else {
            warn!("relay skips {} in {collection}: no size in meta", link.0);
            continue;
        };
        let split = kind.split_link_id(link);
        out.push(RelayTarget {
            holding,
            handle: binding.handle.clone(),
            link: link.0.clone(),
            hint: split.hint.map(str::to_string),
            mint: split.mint.map(str::to_string),
            size,
            flags: to_email_flag_set(&item.flags).into_iter().collect(),
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

/// Streams each cross-copy body directly from its holding side to the other
/// through a bounded pipe, the store keeping only the spine: the target's next
/// enumerate binds the relayed message, whose body is never stored. Returns how
/// many were relayed.
///
/// A relay never reaches the projection the hydrating path is reported from, so
/// each write to the target server is itemized here instead. Without that, a
/// run that relayed would report having written nothing.
fn relay_copies(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    progress: &CollectionProgress,
    report: &mut SyncReport,
) -> Result<usize> {
    let targets = relay_targets(
        store,
        resolve_kind(&mut left.pool),
        collection,
        (&left.name, left.perms.item.create),
        (&right.name, right.perms.item.create),
    )?;
    let display = display_name(&left.namespace, collection).to_string();
    let total = targets.len();
    let mut count = 0;

    for target in targets {
        let holds_left = target.holding == left.name;
        let (holding_pool, target_pool) = if holds_left {
            (&mut left.pool, &mut right.pool)
        } else {
            (&mut right.pool, &mut left.pool)
        };
        let target_name = if holds_left {
            right.name.clone()
        } else {
            left.name.clone()
        };

        relay_one(holding_pool, target_pool, collection, &target)
            .with_context(|| format!("Relay {} in {display}", target.handle.0))?;

        report.item.patch.push(PatchEntry::new(
            ItemHunk::Copy {
                source_side: target.holding.clone(),
                target_side: target_name,
                collection: display.clone(),
                source_id: target.link.clone(),
                flags: target.flags.iter().cloned().collect(),
                content_key: content_key(&target.link),
            },
            None,
        ));
        count += 1;
        progress.tick(count, total);
    }
    Ok(count)
}

/// Relays one message: a worker thread streams the fetch into the bounded pipe
/// while this thread streams the pipe into the length-prefixed target append,
/// so the body crosses without ever being held whole or stored.
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
            LinkId {
                hint: target.hint.as_deref(),
                mint: target.mint.as_deref(),
            },
        );
        (fetch.join().unwrap(), append)
    });
    fetch.context("relay fetch")?;
    append.context("relay append")?;
    Ok(())
}

/// One per-side reconcile round: sync each side against its server, then
/// resolve any freshly probed placement to `Meta` so its link id is known and
/// it joins the hub. Returns whether either side pulled or pushed.
#[allow(clippy::too_many_arguments)]
fn reconcile_pass(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    store_dir: &Path,
    dry_run: bool,
) -> Result<bool> {
    let left_report = sync_side_rebuilding(
        collection,
        left,
        left_store,
        blobs,
        !dry_run && left.writable(),
    )?;
    upgrade_probed(collection, left, left_store, blobs, dry_run)?;
    resolve_conflicts(collection, left, left_store, blobs, store_dir, dry_run)?;
    let right_report = sync_side_rebuilding(
        collection,
        right,
        right_store,
        blobs,
        !dry_run && right.writable(),
    )?;
    upgrade_probed(collection, right, right_store, blobs, dry_run)?;
    resolve_conflicts(collection, right, right_store, blobs, store_dir, dry_run)?;
    Ok(moved(&left_report) || moved(&right_report))
}

/// Whether a side's sync changed anything (pulled or the remote accepted a
/// push), used to detect convergence.
fn moved(report: &ReplicaSyncReport) -> bool {
    report.pulled > 0 || report.pushed > 0
}

/// Runs one side's sync, then the handle-space rebuild guard: the stored
/// checkpoint's epoch is read before and after the sync, and a change means the
/// server renumbered every handle. io-replica's rekey then re-enumerates the
/// new handle space, carrying cached bodies, summaries and pending state over
/// by link id, and its write batch lands through
/// [`PimdirSourceStore::write_rekeyed`] so `collections.generation` bumps
/// atomically with the rebuild. Ordinary syncs and full resyncs never bump.
/// Graph sides never rebuild, their message ids surviving a delta reset.
///
/// The guard is pre/post within one run, neverest keeping no
/// per-collection state beside the store. A crash between the sync's checkpoint
/// write and the rekey loses the pre value, so that window can miss one
/// generation bump; content still converges through link ids on the next sync.
fn sync_side_rebuilding(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    push: bool,
) -> Result<ReplicaSyncReport> {
    let pre = stored_epoch(ctx.pool.primary(), store, collection)?;

    let report = sync_side(collection, ctx, store, blobs, push)?;

    if let Some(pre) = pre
        && let Some(post) = stored_epoch(ctx.pool.primary(), store, collection)?
        && post != pre
    {
        info!("handle-space epoch of {collection} changed, rebuilding the collection");
        let (rekey, generation) = rebuild_collection(collection, ctx, store, blobs)?;
        info!(
            "{collection} rebuilt under generation {generation}: {} carried, {} pulled, {} pending dropped",
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
fn stored_epoch(
    client: &Client,
    store: &PimdirSourceStore,
    collection: &str,
) -> Result<Option<u64>> {
    let loaded = store
        .load(
            &ReplicaCollectionId(collection.to_string()),
            &ReplicaLoadScope::All,
        )
        .map_err(|err| anyhow!("Load {collection} checkpoint error: {err}"))?;
    Ok(loaded
        .checkpoint
        .as_ref()
        .and_then(|checkpoint| client.handle_space_epoch(&checkpoint.0)))
}

/// Drives io-replica's rekey over the side's remote, routing its rebuild
/// write batch through [`PimdirSourceStore::write_rekeyed`] instead of the
/// plain storage seam, so "the ids you cached are void" (the generation
/// bump) commits atomically with the rebuild that voided them. Returns
/// the rekey report and the new generation.
fn rebuild_collection(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
) -> Result<(ReplicaRekeyReport, i64)> {
    let mut remote = PimRemote::new(&mut ctx.pool, blobs.clone(), ctx.namespace.clone());
    drive_rekey(store, &mut remote, collection)
}

/// The generic rekey pump behind [`rebuild_collection`], seam-typed so a
/// test can drive it over a scripted remote.
fn drive_rekey<R>(
    store: &mut PimdirSourceStore,
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
                let generation = generation.context("Rekey completed without a write")?;
                return Ok((report, generation));
            }
            ReplicaCoroutineState::Complete(Err(err)) => {
                return Err(anyhow!("Rekey engine error: {err}"));
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsLoad { collection, scope }) => {
                let loaded = store
                    .load(&collection, &scope)
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
                    .map_err(|err| anyhow!("Remote enumerate error: {err:#}"))?;
                arg = Some(ReplicaArg::Enumerate(snapshot));
            }
            ReplicaCoroutineState::Yielded(ReplicaYield::WantsFetch {
                collection,
                handles,
                tier,
            }) => {
                let items = remote
                    .fetch(&collection, handles, tier)
                    .map_err(|err| anyhow!("Remote fetch error: {err:#}"))?;
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

/// The engine options one side syncs under.
///
/// Every side here is bound to the store's hub, which is what fixes the delete
/// disposition: a refused delete (`push` off, or `item.delete = false`) is
/// held, never reverted. Reverting one says "this source still holds the
/// member", which a hub reads as the item being alive, since an add beats a
/// delete across sources: it clears the deletion for every side and mirrors the
/// item back to the one it was deleted on. A backup side configured to take no
/// deletes would then resurrect on both what the user removed on one.
fn sync_options(
    push: bool,
    rights: ReplicaPushRights,
    conflict: ReplicaConflictPolicy,
) -> ReplicaSyncOptions {
    ReplicaSyncOptions {
        push,
        rights,
        delete: ReplicaDeletePolicy::Keep,
        conflict,
        ..Default::default()
    }
}

/// Runs one side's `sync` against its server and returns its report.
fn sync_side(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    push: bool,
) -> Result<ReplicaSyncReport> {
    let opts = sync_options(push, ctx.push_rights(), ctx.conflict_policy());
    let mut remote = PimRemote::new(&mut ctx.pool, blobs.clone(), ctx.namespace.clone());
    let report = drive(
        store,
        &mut remote,
        ReplicaSync::new(collection.to_string(), opts),
    );
    // Kept whether the pass succeeded or not: a refusal is something the run
    // did learn, and a later failure does not unlearn it.
    let refused = remote.take_refused();
    ctx.refused.extend(refused);

    report.with_context(|| format!("Sync {} {collection}", &ctx.name))
}

/// Raises every freshly probed placement to the tier its kind resolves at
/// ([`Kind::probe_tier`]) so its link id and summary are known and it enters
/// the hub: `Meta` for mail, whose envelope carries the identity, `Full` for a
/// kind whose body is the only thing that does.
fn upgrade_probed(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    dry_run: bool,
) -> Result<()> {
    let probed: Vec<ReplicaHandle> = load_side(store, collection)
        .with_context(|| format!("Load {} {collection}", &ctx.name))?
        .into_iter()
        .filter(|p| p.level == ReplicaLevel::Probed && p.status != ReplicaStatus::Tombstone)
        .map(|p| p.handle)
        .collect();
    if probed.is_empty() {
        return Ok(());
    }
    let tier = resolve_kind(&mut ctx.pool).probe_tier();

    if dry_run && tier == ReplicaTier::Full {
        return Ok(());
    }

    let mut remote = PimRemote::new(&mut ctx.pool, blobs.clone(), ctx.namespace.clone());
    drive(
        store,
        &mut remote,
        ReplicaUpgrade::new(collection.to_string(), probed, tier),
    )
    .with_context(|| format!("Upgrade probed {} {collection}", &ctx.name))?;
    Ok(())
}

/// One side's placements the engine left marked conflicted in `collection`.
fn conflicted_placements(
    store: &PimdirSourceStore,
    collection: &str,
    source: &str,
) -> Result<Vec<ReplicaPlacement>> {
    Ok(load_side(store, collection)
        .with_context(|| format!("Load {source} {collection}"))?
        .into_iter()
        .filter(|placement| placement.status == ReplicaStatus::Conflict)
        .collect())
}

/// Merges what nobody disagreed about, leaving parked only the divergences a
/// person has to settle. Returns how many conflicts it cleared.
///
/// Most divergence is not disagreement: one side changed a phone number and
/// the other a note, and the base the last sync agreed on proves it by naming
/// which side touched which field. Merging those needs no one, and a
/// background tool that asks anyway is one a user switches off.
///
/// A conflict is marked with the diverging remote body *wanted* rather than
/// held, the engine fetching nothing by itself, so the first half of this is
/// an ordinary `Full` upgrade of the conflicted placements: what it fetches
/// lands on the conflict object and nowhere else, the placement's own body
/// being the local side of the divergence. A conflict whose remote body has
/// not landed yet is visible and not resolvable, and is left exactly as it is
/// rather than merged against a body nobody holds.
///
/// The merge is [`Kind::merge`], dispatched on the collection's kind and
/// built in rather than configured, and the resolution is staged as an
/// ordinary `update` through the store's queue then drained in the same
/// breath. That is already the path whoever owns an edit resolves a conflict
/// by, so a merged body is written exactly one way. Anything the merge did
/// not settle stays parked, untouched.
fn resolve_conflicts(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    store_dir: &Path,
    dry_run: bool,
) -> Result<usize> {
    if dry_run {
        return Ok(0);
    }

    let parked = conflicted_placements(store, collection, &ctx.name)?;
    if parked.is_empty() {
        return Ok(0);
    }

    debug!("merge {} conflicted item(s) in {collection}", parked.len());

    let wanted: Vec<ReplicaHandle> = parked
        .iter()
        .filter(|placement| placement.conflict_object.is_none())
        .map(|placement| placement.handle.clone())
        .collect();

    if !wanted.is_empty() {
        let mut remote = PimRemote::new(&mut ctx.pool, blobs.clone(), ctx.namespace.clone());
        drive(
            store,
            &mut remote,
            ReplicaUpgrade::new(collection.to_string(), wanted, ReplicaTier::Full),
        )
        .with_context(|| format!("Fetch the diverging bodies of {} {collection}", &ctx.name))?;
    }

    let kind = resolve_kind(&mut ctx.pool);

    merge_conflicts(collection, kind, &ctx.name, store, blobs, store_dir)
}

/// The half of [`resolve_conflicts`] that reaches no server: merges the
/// conflicted placements whose three bodies the store already holds, and
/// stages each empty report as an ordinary edit. Returns how many it cleared.
fn merge_conflicts(
    collection: &str,
    kind: Kind,
    source: &str,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    store_dir: &Path,
) -> Result<usize> {
    // NOTE: opened before the first blob write rather than at it, the
    // producer's staging lock being what keeps a collector out of the window
    // between a body reaching the blob tree and the queue row pinning it.
    let mut producer = PimdirProducer::open(store_dir, env!("CARGO_PKG_NAME"))
        .with_context(|| format!("Stage the merged conflicts of {collection}"))?;
    let mut staged = 0usize;

    for placement in conflicted_placements(store, collection, source)? {
        let handle = placement.handle.0.clone();

        let Some(link_id) = placement.link_id.clone() else {
            debug!("conflicted item {handle} in {collection} carries no link id yet");
            continue;
        };

        let sides = (
            placement.base.as_ref().and_then(|base| base.object.clone()),
            placement.object.clone(),
            placement.conflict_object.clone(),
        );
        let (Some(base_hash), Some(local_hash), Some(remote_hash)) = sides else {
            debug!("conflicted item {handle} in {collection} is missing a side to merge against");
            continue;
        };

        let read = |hash: &ReplicaHash| -> Result<Option<Vec<u8>>> {
            blobs.get(hash).with_context(|| {
                format!(
                    "Read the body {} of {handle} in {collection}",
                    hash.as_str()
                )
            })
        };
        let (Some(base), Some(local), Some(remote)) =
            (read(&base_hash)?, read(&local_hash)?, read(&remote_hash)?)
        else {
            debug!("a body of the conflicted item {handle} in {collection} is not in the store");
            continue;
        };

        let body = match kind.merge(&base, &local, &remote) {
            Merged::Body(body) => body,
            Merged::Collided(fields) => {
                debug!("both sides changed {fields} field(s) of {handle} in {collection}");
                continue;
            }
            Merged::Unmergeable(why) => {
                debug!("cannot merge {handle} in {collection}: {why}");
                continue;
            }
        };

        let Some(seq) = store
            .seq_for_link(collection, &link_id.0)
            .with_context(|| format!("Resolve the id of {handle} in {collection}"))?
        else {
            debug!("conflicted item {handle} in {collection} has no row to update");
            continue;
        };

        let hash = blobs.hash(&body);
        let mut writer = blobs
            .writer()
            .with_context(|| format!("Store the merged body of {handle} in {collection}"))?;
        writer
            .write_all(&body)
            .with_context(|| format!("Store the merged body of {handle} in {collection}"))?;
        let size = writer
            .commit(&hash)
            .with_context(|| format!("Store the merged body of {handle} in {collection}"))?;

        let (_, meta, _) = kind.parse_body(&body, size);
        producer
            .enqueue(
                collection,
                &PimdirAction::Update {
                    seq,
                    object: hash,
                    meta: Some(meta),
                },
                Some(size),
                &Utc::now().to_rfc3339(),
            )
            .with_context(|| format!("Stage the merged body of {handle} in {collection}"))?;

        staged += 1;
    }

    drop(producer);

    if staged == 0 {
        return Ok(0);
    }

    let drained = store
        .drain_collection(collection)
        .with_context(|| format!("Apply the merged conflicts of {collection}"))?;
    if drained.parked > 0 {
        warn!(
            "{} merged conflict(s) in {collection} could not be applied and parked",
            drained.parked
        );
    }
    info!("merged and resolved {staged} conflict(s) in {collection}");

    Ok(staged)
}

/// Hydrates (to `Full`) the bodies of items held by only one side that the other
/// side may receive, so the hub holds the body and its next projection stages
/// the copy. Returns how many bodies were fetched.
fn hydrate_copies(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    progress: &CollectionProgress,
) -> Result<usize> {
    let targets = hydration_targets(
        left_store,
        collection,
        (&left.name, left.perms.item.create),
        (&right.name, right.perms.item.create),
    )
    .with_context(|| format!("Hydration targets {collection}"))?;
    if targets.is_empty() {
        return Ok(0);
    }

    let mut left_handles = Vec::new();
    let mut right_handles = Vec::new();
    for (source, handle) in &targets {
        if *source == left.name {
            left_handles.push(handle.clone());
        } else {
            right_handles.push(handle.clone());
        }
    }
    let total = targets.len();
    let done = AtomicUsize::new(0);
    let tick = || progress.tick(done.fetch_add(1, Ordering::Relaxed) + 1, total);

    if !left_handles.is_empty() {
        let mut remote = PimRemote::with_progress(
            &mut left.pool,
            blobs.clone(),
            left.namespace.clone(),
            &tick,
            HashMap::new(),
        );
        drive(
            left_store,
            &mut remote,
            ReplicaUpgrade::new(collection.to_string(), left_handles, ReplicaTier::Full),
        )
        .with_context(|| format!("Hydrate bodies for {} {collection}", left.name))?;
    }
    if !right_handles.is_empty() {
        let mut remote = PimRemote::with_progress(
            &mut right.pool,
            blobs.clone(),
            right.namespace.clone(),
            &tick,
            HashMap::new(),
        );
        drive(
            right_store,
            &mut remote,
            ReplicaUpgrade::new(collection.to_string(), right_handles, ReplicaTier::Full),
        )
        .with_context(|| format!("Hydrate bodies for {} {collection}", right.name))?;
    }
    Ok(total)
}

/// Itemizes the pending cross-side work into the report by reading each side's
/// hub projection: a `Created` placement is a copy in, a `Dirty` one a flag
/// change, a `Tombstone` a delete.
fn itemize(
    collection: &str,
    store: &PimdirStore,
    left: &SourceCtx,
    right: &SourceCtx,
    report: &mut SyncReport,
) -> Result<()> {
    for (ctx, other) in [(left, right), (right, left)] {
        let display = display_name(&ctx.namespace, collection);
        let view = projection_view(store, collection, &ctx.name)
            .with_context(|| format!("Project {} {display}", ctx.name))?;
        for placement in view {
            for hunk in placement_hunks(&ctx.name, &other.name, display, &placement) {
                report.item.patch.push(PatchEntry::new(hunk, None));
            }
        }
    }
    Ok(())
}

/// Maps a projected placement to its report hunks. A flag change can surface as
/// both an add and a remove.
fn placement_hunks(
    source: &str,
    other: &str,
    collection: &str,
    placement: &ReplicaPlacement,
) -> Vec<ItemHunk> {
    match placement.status {
        ReplicaStatus::Created => {
            let link = placement
                .link_id
                .as_ref()
                .map(|l| l.0.clone())
                .unwrap_or_default();
            vec![ItemHunk::Copy {
                source_side: other.to_string(),
                target_side: source.to_string(),
                collection: collection.to_string(),
                source_id: link.clone(),
                flags: to_email_flag_set(&placement.flags),
                content_key: content_key(&link),
            }]
        }
        ReplicaStatus::Tombstone => vec![ItemHunk::Delete {
            side: source.to_string(),
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
                    side: source.to_string(),
                    collection: collection.to_string(),
                    id: placement.handle.0.clone(),
                    flags: added,
                    content_key: 0,
                });
            }
            if !removed.is_empty() {
                hunks.push(ItemHunk::RemoveFlags {
                    side: source.to_string(),
                    collection: collection.to_string(),
                    id: placement.handle.0.clone(),
                    flags: removed,
                    content_key: 0,
                });
            }
            let base_object = placement.base.as_ref().and_then(|b| b.object.as_ref());
            if placement.object.as_ref() != base_object {
                hunks.push(ItemHunk::Update {
                    side: source.to_string(),
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

/// Names the creates a side refused because it already holds the identity, in
/// the terms the report speaks: the source's configured name, the collection
/// as its server names it, and the shared `UID`.
///
/// The remote collects them as it pushes ([`PimRemote::take_refused`]) and
/// leaves them on the side's context; what is added here is the side's name,
/// which the remote does not know it by.
fn itemize_refused(side: &str, refused: Vec<RefusedCreate>, report: &mut SyncReport) {
    report
        .refused
        .extend(refused.into_iter().map(|refused| RefusedDuplicate {
            side: side.to_string(),
            collection: refused.collection,
            uid: refused.uid,
        }));
}

/// The context of the named source in a pair.
fn source_ctx<'a>(
    name: &str,
    left: &'a mut SourceCtx,
    right: &'a mut SourceCtx,
) -> &'a mut SourceCtx {
    if left.name == name { left } else { right }
}

fn list_collections(client: &mut Client) -> Result<HashSet<String>> {
    Ok(client
        .list_collections(false)
        .context("List collections error")?
        .into_iter()
        .map(|m| m.name)
        .collect())
}

fn filter_collections(
    collections: &HashSet<String>,
    filter: &CollectionFilter,
) -> BTreeSet<String> {
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
fn diff_collections(
    left: &BTreeSet<String>,
    right: &BTreeSet<String>,
    left_ctx: &SourceCtx,
    right_ctx: &SourceCtx,
) -> Vec<CollectionHunk> {
    let mut hunks = Vec::new();
    for collection in left.difference(right) {
        if right_ctx.perms.collection.create {
            hunks.push(CollectionHunk::Create {
                side: right_ctx.name.clone(),
                collection: collection.clone(),
            });
        }
    }
    for collection in right.difference(left) {
        if left_ctx.perms.collection.create {
            hunks.push(CollectionHunk::Create {
                side: left_ctx.name.clone(),
                collection: collection.clone(),
            });
        }
    }
    hunks
}

fn apply_collection_hunk(
    hunk: &CollectionHunk,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
) -> Result<()> {
    match hunk {
        CollectionHunk::Create { side, collection } => {
            source_ctx(side, left, right)
                .pool
                .primary()
                .create_collection(collection)
                .with_context(|| format!("Create collection {collection} on {side}"))?;
        }
        CollectionHunk::Scan { .. } => {
            unreachable!("a scan hunk reports a failure and is never applied")
        }
        CollectionHunk::Delete { side, collection } => {
            source_ctx(side, left, right)
                .pool
                .primary()
                .delete_collection(collection)
                .with_context(|| format!("Delete collection {collection} on {side}"))?;
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

/// A stable-ish u64 content key for the report DTO. Display never shows it, so
/// it only needs to be internally consistent.
fn content_key(link: &str) -> u64 {
    let mut acc: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in link.bytes() {
        acc ^= byte as u64;
        acc = acc.wrapping_mul(0x0000_0100_0000_01b3);
    }
    acc
}

/// Drains the pending frontend actions of the collections `namespace` owns
/// into the store before the sync, neverest being the store's sole owner. Each
/// collection is applied exactly once by io-pimdir's
/// [`drain_collection`](PimdirSourceStore::drain_collection). A permanently bad
/// action parks, for [`report_parked`] to surface until it is repaired; a
/// transient failure leaves the collection queued for the next run and only
/// warns, the sync itself still running offline-first.
///
/// The queue is the whole store's and records no source, so the collections it
/// names have to be narrowed here: a source drains what its own namespace owns
/// and nothing else. Draining another's projects an action against a source
/// holding no binding for the item it names, which io-pimdir leaves pending
/// rather than parking, but only after the drain that could have applied it
/// has been robbed of its turn. Sources are run in name order, so without this
/// the first one alphabetically would answer for every frontend write on the
/// account.
fn drain_queues(store: &mut PimdirSourceStore, namespace: &str, report: &mut SyncReport) {
    let collections = match store.queued_collections() {
        Ok(collections) => collections,
        Err(err) => {
            warn!("cannot list queued collections: {err}");
            return;
        }
    };

    let prefix = format!("{namespace}/");

    for collection in collections {
        if !collection.starts_with(&prefix) {
            continue;
        }
        match store.drain_collection(&collection) {
            Ok(drained) => {
                if drained.applied > 0 || drained.parked > 0 || drained.skipped > 0 {
                    info!(
                        "drained {} queued action(s) in {collection} ({} parked, {} skipped)",
                        drained.applied, drained.parked, drained.skipped
                    );
                }
                if drained.applied > 0 {
                    report.drained.push(DrainedQueue {
                        collection: collection.clone(),
                        applied: drained.applied,
                    });
                }
            }
            Err(err) => warn!("drain of {collection} failed, actions stay queued: {err}"),
        }
    }
}

/// Surfaces the store's parked queue actions, once for the run.
///
/// A parked row belongs to the store, not to a source: it is whatever a
/// frontend enqueued and the drain found permanently unappliable, and the
/// queue records no source. Reading it where the drain runs would report it
/// once per source that ran, so a mail account syncing contacts and calendar
/// beside its mail would show the same row three times.
///
/// They are re-reported by every run, until the row is repaired or dropped.
fn report_parked(store: &PimdirStore, report: &mut SyncReport) {
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

/// Counts the decisions the store is holding, once for the run. That count is
/// what the report states and what the run's exit code answers.
///
/// Read from the store rather than from the run's own tally, because the two
/// are different numbers. The engine emits nothing for a placement it already
/// parked, so the conflicts a run itemizes are only the ones it newly marked;
/// that early return is what keeps a repeated run quiet, and it is exactly
/// why it cannot also serve as the number of decisions waiting.
fn count_conflicts(store: &PimdirStore, account: &str, report: &mut SyncReport) {
    match store.list_conflicts(Some(account)) {
        Ok(conflicts) => report.outstanding_conflicts = conflicts.len(),
        Err(err) => warn!("cannot count the conflicts waiting for a decision: {err}"),
    }
}

/// Announces the conflicts this run marked: a warning per item in the log,
/// then the account's notification once, if it declares one.
///
/// The log is the default and the notification is the opt-in, so an
/// unattended run never shells out unasked. Only what this run marked is
/// announced, which is the whole of the once-only rule: the engine returns
/// early for a placement it already parked, so a five-minute schedule over one
/// unresolved card raises one notification rather than nearly three hundred a
/// day, all naming the same card. An unattended tool that repeats itself is
/// one a user silences.
///
/// One notification carries the run rather than one per item, a dozen popups
/// answering the same question being a dozen too many. What it says is the
/// configuration's to write, the items themselves being in the report and in
/// the log.
fn announce_conflicts(account_config: &AccountConfig, report: &SyncReport) {
    if report.conflicts.is_empty() {
        return;
    }

    for conflict in &report.conflicts {
        warn!("{conflict}");
    }

    let Some(notification) = &account_config.conflict.notify else {
        return;
    };

    if let Err(err) = notification.0.show() {
        warn!("cannot show the conflict notification: {err}");
    }
}

/// Performs the queue's `submit` intents, the half the store's own drain leaves
/// pending because performing one is a capability rather than a mutation.
///
/// Each intent goes through the first side offering a send channel: its own
/// `smtp` table when it carries one, else its native send. A sent intent is
/// acknowledged, which releases its body's pin; a permanent failure parks the
/// row with its error; a transient one leaves it pending for the next run.
/// Without a channel at all the intents stay pending with a warning, never
/// parked, since another build or another host can perform them.
fn drain_submits(
    account_config: &AccountConfig,
    account: &Account,
    sides: &mut [&mut SourceCtx],
    store: &mut PimdirSourceStore,
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
        let Some(mut channel) = open_send_channel(account_config, account, sides, intents.len())
        else {
            return;
        };

        let mut sent = 0;
        for intent in &intents {
            let subject = intent.subject();
            let entry = match submit::send_one(&mut channel, blobs, intent) {
                Ok(()) => {
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
        let _ = (account_config, account, sides, blobs, store, report);
        warn!(
            "this build has no send channel (needs the `smtp` or the `msgraph` cargo feature), {} submit intent(s) stay pending",
            intents.len()
        );
    }
}

/// Reclaims the store's retained (soft-deleted) items older than
/// `store.purge-after`. It runs after the sync rather than before it, so an
/// item this run retired starts its delay now instead of being reclaimed by
/// the very run that retired it.
///
/// Unset means never purge, so the sweep does not run at all. It warns rather
/// than fails: a store that cannot be swept is a housekeeping problem, not a
/// reason to fail a run that synced correctly.
///
/// A purge releases a body, it does not reclaim one. The store collects nothing
/// by itself (pimdir SPEC §5), so the store's owner runs the collector, and
/// here that is this. It runs only after a purge that took something, since
/// that is when this run knows a body was released and the collector costs a
/// walk of the whole blob tree. Orphans left by a crash are what `pimdir gc`
/// is for.
fn sweep_retained(
    account_config: &AccountConfig,
    store: &mut PimdirStore,
    report: &mut SyncReport,
) {
    let Some(cutoff) = account_config.store.purge_cutoff(Utc::now()) else {
        return;
    };

    let purged = match store.purge_retained_before(&cutoff) {
        Ok(purged) => purged,
        Err(err) => return warn!("retention sweep failed, nothing was purged: {err}"),
    };

    let collected = match purged.items {
        0 => Default::default(),
        _ => match store.collect_garbage() {
            Ok(collected) => collected,
            Err(err) => {
                warn!("purged items but collected nothing: {err}");
                Default::default()
            }
        },
    };

    if purged.items > 0 {
        info!(
            "purged {} retained item(s) older than {cutoff}, collected {} object(s), {} byte(s) reclaimed",
            purged.items, collected.objects, collected.bytes
        );
    }

    report.purged = Some(PurgedItems {
        items: purged.items,
        objects: collected.objects,
        bytes: collected.bytes,
    });
}

/// Resolves the account's send channel. It belongs to a source: its own `smtp`
/// table when it carries one, else its native send. At most one source may
/// declare an `smtp` table (the account refuses more at load), so there is no
/// tiebreak here. `None` warns and leaves the queued intents pending.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
#[cfg_attr(not(feature = "smtp"), allow(unused_variables))]
fn open_send_channel<'a>(
    account_config: &AccountConfig,
    account: &Account,
    sides: &'a mut [&mut SourceCtx],
    queued: usize,
) -> Option<submit::SendChannel<'a>> {
    let configured = account_config.sources().ok()?;
    let pick = sides.iter().enumerate().find_map(|(index, ctx)| {
        let source = configured.get(&ctx.name)?;
        match &source.smtp {
            Some(_) => Some(SendChannelPick::Smtp(ctx.name.clone())),
            None if source.sends_natively() => Some(SendChannelPick::Native(index)),
            None => None,
        }
    });

    match pick {
        #[cfg(feature = "smtp")]
        Some(SendChannelPick::Smtp(name)) => {
            let opened = account.get(&name).and_then(|source| {
                let smtp = source.smtp.expect("the pick found a configured channel");
                submit::connect_smtp(&smtp)
            });

            match opened {
                Ok(client) => Some(submit::SendChannel::Smtp(client)),
                Err(err) => {
                    warn!("cannot open the smtp channel, {queued} intent(s) stay pending: {err:#}");
                    None
                }
            }
        }
        #[cfg(not(feature = "smtp"))]
        Some(SendChannelPick::Smtp(_)) => {
            warn!(
                "the source's `smtp` table needs the `smtp` cargo feature, {queued} intent(s) stay pending"
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

/// Which side performs the submit intents, and how: its own SMTP channel, or
/// the live session of the side at that index, a native sender.
///
/// Each variant is consumed by the arm its cargo feature gates, so a
/// build with only one of them carries the other unread rather than duplicating
/// the walk per feature combination.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
#[allow(dead_code)]
/// Which source completes the account's submission, and how.
enum SendChannelPick {
    /// The name of the source whose `smtp` table the intents leave through.
    Smtp(String),
    /// The index of the source that sends by itself.
    Native(usize),
}

/// Hydrates every not-yet-`Full`, non-tombstone placement of both sides to
/// `Full` under `store.hydration = "full"`, so the store mirrors every body.
/// The shared object store dedups, so the second side's upgrade links an
/// already-stored body by link id without a fetch. Returns how many placements
/// were raised.
#[allow(clippy::too_many_arguments)]
fn hydrate_full_collection(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    progress: &CollectionProgress,
) -> Result<usize> {
    let mut raised = 0;
    for (ctx, store) in [(left, left_store), (right, right_store)] {
        let targets: Vec<ReplicaHandle> = load_side(store, collection)
            .with_context(|| format!("Load {} {collection}", &ctx.name))?
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
        let mut remote = PimRemote::with_progress(
            &mut ctx.pool,
            blobs.clone(),
            ctx.namespace.clone(),
            &tick,
            HashMap::new(),
        );
        drive(
            store,
            &mut remote,
            ReplicaUpgrade::new(collection.to_string(), targets, ReplicaTier::Full),
        )
        .with_context(|| format!("Hydrate all bodies {} {collection}", &ctx.name))?;
        raised += total;
    }
    Ok(raised)
}

/// The blob subdirectory of a pimdir store (SPEC §5), the one part of it
/// a dry run shares with the real store rather than copying.
const BLOBS_DIR: &str = "objects";

/// A throwaway replica of the pimdir store, so a dry run advances no
/// checkpoint and writes to no server, removed however the run ends.
///
/// It is built **beside** the real store rather than under the temporary
/// directory, so the two share a filesystem and the bodies can be
/// hardlinked instead of copied. That is the difference between reading
/// a mail account's whole blob tree and creating a few thousand
/// directory entries, and on a machine whose `/tmp` is a tmpfs it is
/// also the difference between spending gigabytes of memory and
/// spending none.
///
/// Only the blob tree is shared. Everything else, the SQLite database
/// above all, is copied, because a dry run writes to it and a hardlink
/// would carry those writes into the real store. A file this misjudges
/// is copied rather than shared, so the cost of the rule being wrong is
/// a slower dry run and never a corrupted store.
struct DryRunReplica {
    /// Where the replica lives, which is the run's `work_dir`.
    dir: PathBuf,
}

impl DryRunReplica {
    /// Clones the store at `real_dir` into a sibling directory, first
    /// clearing whatever an earlier run left behind: a panic aborts a
    /// release build, where no destructor runs, so a leftover is a state
    /// this has to meet rather than one it can rule out.
    fn new(real_dir: &Path) -> Result<Self> {
        let parent = real_dir.parent().unwrap_or(Path::new("."));
        let name = real_dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("store"));
        let prefix = format!(".{name}-dry-");

        clear_stale_replicas(parent, &prefix);

        let dir = parent.join(format!("{prefix}{}", process::id()));
        fs::create_dir_all(&dir)
            .with_context(|| format!("Create dry-run replica {} error", dir.display()))?;

        if real_dir.exists() {
            let start = Instant::now();
            let mut counts = CloneCounts::default();
            let blobs = real_dir.join(BLOBS_DIR);

            clone_dir(real_dir, &dir, &blobs, &mut counts)
                .with_context(|| format!("Clone {} for dry-run", real_dir.display()))?;

            debug!(
                "dry-run replica prepared in {:?} ({} linked, {} copied)",
                start.elapsed(),
                counts.linked,
                counts.copied
            );

            // A blob tree that could not be shared was read and written
            // whole, which is the slow dry run this exists to avoid.
            if counts.linked == 0 && counts.copied > 0 {
                info!(
                    "dry-run replica copied {} file(s), links unavailable",
                    counts.copied
                );
            }
        }

        Ok(Self { dir })
    }
}

impl Drop for DryRunReplica {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_dir_all(&self.dir) {
            warn!(
                "cannot remove the dry-run replica {}: {err}",
                self.dir.display()
            );
        }
    }
}

/// What [`clone_dir`] did, for the line reporting it.
#[derive(Default)]
struct CloneCounts {
    linked: usize,
    copied: usize,
}

/// Removes the replicas of earlier runs, named by `prefix` under `dir`.
/// A directory that will not go is left with a warning: it costs disk,
/// never correctness, the run about to start writing under a name of its
/// own.
fn clear_stale_replicas(dir: &Path, prefix: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with(prefix) {
            continue;
        }

        let stale = entry.path();
        match fs::remove_dir_all(&stale) {
            Ok(()) => warn!("removed a dry-run replica an earlier run left behind"),
            Err(err) => warn!("cannot remove {}: {err}", stale.display()),
        }
    }
}

/// Clones `src`'s contents into `dst`, created on demand: a file under
/// `blobs` is hardlinked, anything else is copied, and a link the
/// filesystem refuses falls back to a copy.
fn clone_dir(src: &Path, dst: &Path, blobs: &Path, counts: &mut CloneCounts) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());

        if entry.file_type()?.is_dir() {
            clone_dir(&from, &to, blobs, counts)?;
        } else if from.starts_with(blobs) && fs::hard_link(&from, &to).is_ok() {
            counts.linked += 1;
        } else {
            fs::copy(&from, &to)?;
            counts.copied += 1;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use io_replica::{
        change::{ReplicaChange, ReplicaChangeKind, ReplicaDropReason, ReplicaWriteOp},
        collection::ReplicaCheckpoint,
        object::ReplicaObject,
        placement::{ReplicaBase, ReplicaLinkId, ReplicaSortKey},
        remote::{
            ReplicaFetchedBody, ReplicaFetchedItem, ReplicaPushOutcome, ReplicaPushResult,
            ReplicaRemoteItem, ReplicaRemoteSnapshot,
        },
    };

    use super::*;

    /// A store with one body and the files a dry run writes to.
    fn stub_store(dir: &Path) {
        let body = dir.join(BLOBS_DIR).join("ab").join("cd");
        fs::create_dir_all(&body).unwrap();
        fs::write(body.join("abcd"), b"body").unwrap();
        fs::write(dir.join("pimdir.db"), b"index").unwrap();
        fs::write(dir.join("neverest.json"), b"{}").unwrap();
    }

    /// The rule the replica rests on: a body is shared, so a mail
    /// account's blob tree is not read and written whole, and everything
    /// a dry run writes to is its own, so those writes cannot reach the
    /// real store.
    #[cfg(unix)]
    #[test]
    fn a_dry_run_replica_shares_the_bodies_and_copies_the_rest() {
        use std::os::unix::fs::MetadataExt;

        let root = tempfile::tempdir().unwrap();
        let store = root.path().join("account");
        stub_store(&store);

        let replica = DryRunReplica::new(&store).unwrap();

        let body = |dir: &Path| dir.join(BLOBS_DIR).join("ab").join("cd").join("abcd");
        assert_eq!(
            fs::metadata(body(&store)).unwrap().ino(),
            fs::metadata(body(&replica.dir)).unwrap().ino(),
        );
        assert_ne!(
            fs::metadata(store.join("pimdir.db")).unwrap().ino(),
            fs::metadata(replica.dir.join("pimdir.db")).unwrap().ino(),
        );
        assert_eq!(
            fs::read(replica.dir.join("pimdir.db")).unwrap(),
            b"index".to_vec()
        );
    }

    /// Writing to the replica's index leaves the real one alone, which is
    /// what a dry run means and what a shared index would break.
    #[test]
    fn writing_to_a_dry_run_replica_leaves_the_store_alone() {
        let root = tempfile::tempdir().unwrap();
        let store = root.path().join("account");
        stub_store(&store);

        let replica = DryRunReplica::new(&store).unwrap();
        fs::write(replica.dir.join("pimdir.db"), b"advanced").unwrap();

        assert_eq!(
            fs::read(store.join("pimdir.db")).unwrap(),
            b"index".to_vec()
        );
    }

    /// The replica goes however the run ends, an early return included,
    /// which is why it is a guard rather than a line on the way out.
    #[test]
    fn a_dry_run_replica_is_removed_when_the_run_ends() {
        let root = tempfile::tempdir().unwrap();
        let store = root.path().join("account");
        stub_store(&store);

        let dir = {
            let replica = DryRunReplica::new(&store).unwrap();
            replica.dir.clone()
        };

        assert!(!dir.exists());
        assert!(store.join("pimdir.db").exists());
    }

    /// A release build aborts on a panic, running no destructor, so the
    /// next run is what clears what the aborted one left.
    #[test]
    fn a_replica_an_earlier_run_left_behind_is_cleared() {
        let root = tempfile::tempdir().unwrap();
        let store = root.path().join("account");
        stub_store(&store);

        let stale = root.path().join(".account-dry-424242");
        fs::create_dir_all(&stale).unwrap();
        fs::write(stale.join("pimdir.db"), b"leftover").unwrap();

        let _replica = DryRunReplica::new(&store).unwrap();

        assert!(!stale.exists());
    }

    #[test]
    fn a_side_pair_must_agree_on_its_kind() {
        let mail = "message/rfc822";

        assert_eq!(
            pair_kind(("left", mail), ("right", mail)).unwrap(),
            Kind::Mail
        );

        let err = pair_kind(("left", mail), ("right", "text/vcard"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("text/vcard"), "{err}");
        assert!(err.contains("right"), "{err}");

        let err = pair_kind(("left", ""), ("right", mail))
            .unwrap_err()
            .to_string();
        assert!(err.contains("left"), "{err}");
    }

    /// The queue is the whole store's and records no source, so a source
    /// that drained every collection answered for another's work: on an
    /// account syncing mail, contacts and calendar, `caldav` sorts first
    /// and reached every mail action himalaya queued before `imap` did.
    #[test]
    fn a_source_drains_only_the_collections_of_its_own_namespace() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("imap");
        store
            .ensure_collection("imap/INBOX", "message/rfc822")
            .unwrap();

        let mut producer = PimdirProducer::open(dir.path(), "himalaya").unwrap();
        producer
            .enqueue(
                "imap/INBOX",
                &PimdirAction::Add {
                    link_id: Some(ReplicaLinkId("mid:queued@x".into())),
                    flags: ReplicaFlags::default(),
                    object: None,
                    meta: None,
                    handle: None,
                },
                None,
                "2026-08-28T00:00:00Z",
            )
            .unwrap();

        let mut report = SyncReport::default();
        drain_queues(&mut store, "caldav", &mut report);
        assert!(
            report.drained.is_empty(),
            "a mail collection is not caldav's"
        );

        drain_queues(&mut store, "imap", &mut report);
        assert_eq!(report.drained.len(), 1);
        assert_eq!(report.drained[0].collection, "imap/INBOX");
        assert_eq!(report.drained[0].applied, 1);
    }

    /// A parked row belongs to the store, and an account's sources each
    /// drain it: reading the parked rows where the drain runs reported one
    /// row once per source, so a mail account syncing contacts and calendar
    /// beside its mail showed the same warning three times.
    #[test]
    fn a_parked_action_is_reported_once_however_many_sources_drained() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("imap");
        store
            .ensure_collection("imap/INBOX", "message/rfc822")
            .unwrap();

        let mut producer = PimdirProducer::open(dir.path(), "himalaya").unwrap();
        producer
            .enqueue(
                "imap/INBOX",
                &PimdirAction::SetFlags {
                    seq: 6951,
                    flags: ReplicaFlags::from_iter(["\\Seen"]),
                },
                None,
                "2026-08-28T00:00:00Z",
            )
            .unwrap();

        let mut report = SyncReport::default();
        for source in ["caldav", "carddav", "imap"] {
            drain_queues(&mut store, source, &mut report);
        }
        report_parked(&store, &mut report);

        assert_eq!(report.parked.len(), 1);
        assert_eq!(report.parked[0].producer, "himalaya");
    }

    #[test]
    fn the_pre_sync_drain_applies_queued_actions_and_reports_parked_ones() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
        store
            .ensure_collection("left/INBOX", "message/rfc822")
            .unwrap();

        let blobs = store.blobs();
        let mut writer = blobs.writer().unwrap();
        std::io::Write::write_all(&mut writer, b"Subject: queued\r\n\r\nhello").unwrap();
        let hash = ReplicaHash("cafe0001".into());
        let size = writer.commit(&hash).unwrap();

        let mut producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
        producer
            .enqueue(
                "left/INBOX",
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
                "left/INBOX",
                &PimdirAction::Remove { seq: 424242 },
                None,
                "2026-08-07T00:00:01Z",
            )
            .unwrap();
        producer
            .enqueue(
                "left/INBOX",
                &PimdirAction::SetFlags {
                    seq: 424243,
                    flags: ReplicaFlags::from_iter(["\\Seen"]),
                },
                None,
                "2026-08-07T00:00:02Z",
            )
            .unwrap();

        let mut report = SyncReport::default();
        drain_queues(&mut store, "left", &mut report);
        report_parked(&store, &mut report);

        assert_eq!(report.drained.len(), 1);
        assert_eq!(report.drained[0].collection, "left/INBOX");
        assert_eq!(report.drained[0].applied, 2);
        assert_eq!(report.parked.len(), 1);
        assert!(report.parked[0].error.contains("unknown seq"));

        let placements = load_side(&store, "left/INBOX").unwrap();
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].status, ReplicaStatus::Dirty);
        assert!(placements[0].base.is_none());
        assert_eq!(
            placements[0].link_id.as_ref().map(|l| l.0.as_str()),
            Some("mid:q1@x")
        );

        let mut second = SyncReport::default();
        drain_queues(&mut store, "left", &mut second);
        report_parked(&store, &mut second);
        assert!(second.drained.is_empty());
        assert_eq!(second.parked.len(), 1);
    }

    /// The stored checkpoint's UIDVALIDITY, read the way the IMAP adapter
    /// encodes it. The rekey guard itself goes through
    /// `Client::handle_space_epoch`, but this test has no client, so it decodes
    /// directly, which is also why it is gated on the IMAP feature.
    #[cfg(feature = "imap")]
    fn stored_checkpoint_uid_validity(store: &PimdirSourceStore, collection: &str) -> Option<u32> {
        let loaded = store
            .load(
                &ReplicaCollectionId(collection.to_string()),
                &ReplicaLoadScope::All,
            )
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
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
        store.ensure_collection("INBOX", "message/rfc822").unwrap();

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
                    conflict_object: None,
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

        let mut remote = ScriptedRemote {
            spine: vec![(String::from("7"), String::from("mid:a@x"))],
        };
        let (rekey, generation) = drive_rekey(&mut store, &mut remote, "INBOX").unwrap();

        assert_eq!(rekey.rekeyed, 1);
        assert_eq!(rekey.pulled, 0);
        assert_eq!(generation, 2);
        assert_eq!(store.generation("INBOX").unwrap(), Some(2));

        let placements = load_side(&store, "INBOX").unwrap();
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].handle.0, "7");
        assert_eq!(placements[0].object, Some(ReplicaHash("beef0001".into())));
        assert_eq!(placements[0].status, ReplicaStatus::Clean);

        assert_eq!(stored_checkpoint_uid_validity(&store, "INBOX"), Some(2));
    }

    #[test]
    fn no_collection_name_is_reserved() {
        let collections: HashSet<String> = ["INBOX", "Outbox", "Sent"].map(String::from).into();

        let filtered = filter_collections(&collections, &CollectionFilter::All);
        assert_eq!(filtered.len(), 3);
        assert!(filtered.contains("Outbox"));

        let filtered = filter_collections(
            &collections,
            &CollectionFilter::Include(vec![String::from("Outbox")]),
        );
        assert_eq!(filtered, BTreeSet::from([String::from("Outbox")]));
    }

    #[test]
    fn a_submit_intent_survives_the_drain_and_is_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
        store
            .ensure_collection("left/Sent", "message/rfc822")
            .unwrap();

        let blobs = store.blobs();
        let mut writer = blobs.writer().unwrap();
        std::io::Write::write_all(&mut writer, b"Subject: hi\r\n\r\nhello").unwrap();
        let hash = ReplicaHash("cafe0002".into());
        let size = writer.commit(&hash).unwrap();

        let mut producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
        producer
            .enqueue(
                "left/Sent",
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

        let mut report = SyncReport::default();
        drain_queues(&mut store, "left", &mut report);
        assert!(report.drained.is_empty());
        assert!(report.parked.is_empty());
        assert!(load_side(&store, "left/Sent").unwrap().is_empty());

        let intents = submit::pending(&store).unwrap();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].collection, "left/Sent");
        assert_eq!(intents[0].subject().as_deref(), Some("hi"));
        assert_eq!(
            intents[0].object.as_ref().map(|h| h.0.as_str()),
            Some("cafe0002")
        );

        assert!(store.drop_action(intents[0].id).unwrap());
        assert!(submit::pending(&store).unwrap().is_empty());
    }

    #[test]
    fn the_retention_sweep_runs_only_when_a_delay_is_configured() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
        let mut config: AccountConfig =
            toml::from_str(r#"left.imap.server = "imaps://imap.example.org:993""#).unwrap();

        let mut report = SyncReport::default();
        sweep_retained(&config, &mut store, &mut report);
        assert!(report.purged.is_none());
        assert!(config.store.purge_cutoff(Utc::now()).is_none());

        config.store.purge_after = Some(crate::config::HumanDuration(
            std::time::Duration::from_secs(0),
        ));
        sweep_retained(&config, &mut store, &mut report);
        let purged = report.purged.expect("a retention section");
        assert_eq!(purged.items, 0);
        assert_eq!(purged.objects, 0);
        assert_eq!(purged.bytes, 0);
    }

    /// A purged item's body actually leaves the disk.
    ///
    /// A purge releases a reference, it does not reclaim bytes, and the store
    /// collects nothing by itself (pimdir SPEC §5). Without a collector on this
    /// path a backup store would grow without bound while reporting that it had
    /// reclaimed things.
    #[test]
    fn the_sweep_collects_the_bodies_the_purge_released() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = PimdirStore::open(dir.path()).unwrap().for_source("left");

        source
            .write(vec![
                ReplicaWriteOp::StoreObject {
                    object: ReplicaObject {
                        hash: ReplicaHash("beef0002".into()),
                        size: 3,
                    },
                    body: Some(b"abc".to_vec()),
                },
                ReplicaWriteOp::UpsertPlacement(ReplicaPlacement {
                    collection: ReplicaCollectionId("INBOX".into()),
                    handle: ReplicaHandle("1".into()),
                    link_id: Some(ReplicaLinkId("mid:gone@x".into())),
                    object: Some(ReplicaHash("beef0002".into())),
                    level: ReplicaLevel::Full,
                    meta: None,
                    sort_key: ReplicaSortKey::default(),
                    flags: ReplicaFlags::default(),
                    status: ReplicaStatus::Clean,
                    conflict_revision: None,
                    conflict_object: None,
                    base: Some(ReplicaBase {
                        flags: ReplicaFlags::default(),
                        revision: None,
                        object: Some(ReplicaHash("beef0002".into())),
                    }),
                    origin: None,
                }),
            ])
            .unwrap();

        let body = source.blobs().path(&ReplicaHash("beef0002".into()));
        assert!(body.is_file(), "the body was stored");

        source
            .write(vec![ReplicaWriteOp::DropPlacement {
                collection: ReplicaCollectionId("INBOX".into()),
                handle: ReplicaHandle("1".into()),
                reason: ReplicaDropReason::Deleted,
            }])
            .unwrap();
        assert!(body.is_file(), "retention keeps the body");

        // A `0s` delay purges what was retained *strictly* before the
        // cutoff, and the cutoff carries milliseconds, so an item dropped
        // and swept within one of them is not old enough to go. Real
        // delays are days; this test has to age the item itself.
        thread::sleep(std::time::Duration::from_millis(5));

        let mut store = PimdirStore::open(dir.path()).unwrap();
        let config: AccountConfig = toml::from_str(
            r#"
            left.imap.server = "imaps://imap.example.org:993"
            store.purge-after = "0s"
            "#,
        )
        .unwrap();

        let mut report = SyncReport::default();
        drop(source);
        sweep_retained(&config, &mut store, &mut report);

        let purged = report.purged.expect("a retention section");
        assert_eq!(purged.items, 1);
        assert_eq!(purged.objects, 1);
        assert_eq!(purged.bytes, 3);
        assert!(!body.exists(), "the body is gone from the blob tree");
    }

    /// A mutable-content remote: every item carries an ETag, and a write is
    /// conditional on it.
    ///
    /// This is the path no compiled-in backend exercises yet, proven here
    /// before any DAV code exists, so a CardDAV failure later is a protocol bug
    /// rather than an engine one.
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
                let result = match &change.kind {
                    ReplicaChangeKind::Update {
                        handle,
                        object,
                        if_match,
                    } => {
                        let current = self.items.get(&handle.0).map(|(rev, _)| rev.clone());
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

    /// The account a conflicted store is grouped under, which is also what
    /// [`count_conflicts`] scopes its listing to.
    const CONFLICT_ACCOUNT: &str = "cards";

    /// Seeds a store holding one card the engine marked conflicted, with all
    /// three bodies present: the base the last sync agreed on, the local side
    /// of the divergence, and the remote side the upgrade pass supplied.
    ///
    /// This is the state [`merge_conflicts`] starts from, so a test reaches it
    /// without a server: what needs a connection is the fetch that lands the
    /// remote body, and that is the other half of [`resolve_conflicts`].
    fn store_with_conflict(
        dir: &std::path::Path,
        base: &str,
        local: &str,
        remote: &str,
    ) -> PimdirSourceStore {
        let mut store = PimdirStore::open(dir)
            .unwrap()
            .for_account(CONFLICT_ACCOUNT)
            .for_source("dav");
        store.ensure_collection("contacts", "text/vcard").unwrap();

        let blobs = store.blobs();
        let stored = |body: &str| ReplicaWriteOp::StoreObject {
            object: ReplicaObject {
                hash: blobs.hash(body.as_bytes()),
                size: body.len(),
            },
            body: Some(body.as_bytes().to_vec()),
        };

        store
            .write(vec![
                stored(base),
                stored(local),
                stored(remote),
                ReplicaWriteOp::UpsertPlacement(ReplicaPlacement {
                    collection: ReplicaCollectionId("contacts".into()),
                    handle: ReplicaHandle("card1".into()),
                    link_id: Some(ReplicaLinkId("uid:a".into())),
                    object: Some(blobs.hash(local.as_bytes())),
                    level: ReplicaLevel::Full,
                    meta: Some(ReplicaMeta(r#"{"v":1}"#.into())),
                    sort_key: ReplicaSortKey::default(),
                    flags: ReplicaFlags::default(),
                    status: ReplicaStatus::Conflict,
                    conflict_revision: Some(String::from("etag-2")),
                    conflict_object: Some(blobs.hash(remote.as_bytes())),
                    base: Some(ReplicaBase {
                        flags: ReplicaFlags::default(),
                        revision: Some(String::from("etag-1")),
                        object: Some(blobs.hash(base.as_bytes())),
                    }),
                    origin: None,
                }),
            ])
            .unwrap();

        store
    }

    /// A card carrying `tel` and `note`, the two fields the merge tests move
    /// independently of each other.
    fn card(tel: &str, note: &str) -> String {
        format!(
            "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:{tel}\r\nNOTE:{note}\r\nEND:VCARD\r\n"
        )
    }

    /// Two sides editing different fields of one card have said nothing
    /// contradictory: the base names which side touched which, both survive,
    /// the conflict clears through the queue, and the run reports nothing.
    /// Asking a person about this is a background tool asking to be switched
    /// off.
    #[cfg(feature = "merge")]
    #[test]
    fn disjoint_edits_on_both_sides_resolve_with_no_report() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_with_conflict(
            dir.path(),
            &card("+1", "old"),
            &card("+2", "old"),
            &card("+1", "new"),
        );
        let blobs = store.blobs();

        let merged = merge_conflicts(
            "contacts",
            Kind::Vcard,
            "dav",
            &mut store,
            &blobs,
            dir.path(),
        )
        .unwrap();
        assert_eq!(merged, 1);

        let placement = load_side(&store, "contacts").unwrap().remove(0);
        assert_ne!(placement.status, ReplicaStatus::Conflict);
        assert!(placement.conflict_object.is_none());
        assert_eq!(
            placement.base.and_then(|base| base.revision).as_deref(),
            Some("etag-2"),
            "the push the resolution stages is conditioned on the revision it \
             was merged against, not on the one the divergence started from",
        );

        let body = blobs.get(&placement.object.unwrap()).unwrap().unwrap();
        let body = String::from_utf8(body).unwrap();
        assert!(body.contains("TEL:+2"), "{body}");
        assert!(body.contains("NOTE:new"), "{body}");

        let mut report = SyncReport::default();
        itemize_pulled(
            &[ReplicaEvent::Conflicted(ReplicaHandle("card1".into()))],
            &HashMap::new(),
            &store,
            "contacts",
            "contacts",
            "dav",
            &mut report,
        )
        .unwrap();
        assert!(
            report.conflicts.is_empty(),
            "a divergence the run settled is not reported"
        );

        count_conflicts(&store, CONFLICT_ACCOUNT, &mut report);
        assert_eq!(report.outstanding_conflicts, 0);
    }

    /// Both sides setting the same field is the residue no merge settles. It
    /// stays parked and is counted from the store, which is the number the
    /// run's own exit code answers with 2.
    #[cfg(feature = "merge")]
    #[test]
    fn a_same_field_collision_parks_and_is_still_counted() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_with_conflict(
            dir.path(),
            &card("+1", "old"),
            &card("+2", "old"),
            &card("+3", "old"),
        );
        let blobs = store.blobs();
        let local = blobs.hash(card("+2", "old").as_bytes());

        let merged = merge_conflicts(
            "contacts",
            Kind::Vcard,
            "dav",
            &mut store,
            &blobs,
            dir.path(),
        )
        .unwrap();
        assert_eq!(merged, 0);

        let placement = load_side(&store, "contacts").unwrap().remove(0);
        assert_eq!(placement.status, ReplicaStatus::Conflict);
        assert_eq!(placement.object, Some(local), "the local side is untouched");

        let mut report = SyncReport::default();
        itemize_pulled(
            &[ReplicaEvent::Conflicted(ReplicaHandle("card1".into()))],
            &HashMap::new(),
            &store,
            "contacts",
            "contacts",
            "dav",
            &mut report,
        )
        .unwrap();
        assert_eq!(report.conflicts.len(), 1);

        count_conflicts(&store, CONFLICT_ACCOUNT, &mut report);
        assert_eq!(report.outstanding_conflicts, 1);
    }

    /// A conflict an earlier run parked reaches no later run's report: the
    /// engine returns early for a placement it already marked, so it emits no
    /// event and there is nothing to announce. The store still holds the
    /// decision, which is why the two numbers are read from two places.
    #[test]
    fn a_second_run_over_an_unresolved_conflict_announces_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_conflict(
            dir.path(),
            &card("+1", "old"),
            &card("+2", "old"),
            &card("+3", "old"),
        );

        let mut report = SyncReport::default();
        itemize_pulled(
            &[],
            &HashMap::new(),
            &store,
            "contacts",
            "contacts",
            "dav",
            &mut report,
        )
        .unwrap();
        assert!(report.conflicts.is_empty());

        count_conflicts(&store, CONFLICT_ACCOUNT, &mut report);
        assert_eq!(report.outstanding_conflicts, 1);

        let text = report.to_string();
        assert!(text.contains("1 item(s) waiting for a decision"), "{text}");
    }

    /// Seeds a store holding one locally edited item: its body points at
    /// `edited`, its base at `original` and revision `base_revision`, i.e. the
    /// state a frontend leaves behind after staging an edit offline.
    fn store_with_local_edit(dir: &std::path::Path, base_revision: &str) -> PimdirSourceStore {
        let mut store = PimdirStore::open(dir).unwrap().for_source("left");
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
                    conflict_object: None,
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

    /// A store holding one card the client has staged a delete for: the
    /// tombstone a frontend leaves behind after removing an item offline.
    fn store_with_local_delete(dir: &std::path::Path) -> PimdirSourceStore {
        let mut store = PimdirStore::open(dir).unwrap().for_source("left");
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
                ReplicaWriteOp::UpsertPlacement(ReplicaPlacement {
                    collection: ReplicaCollectionId("contacts".into()),
                    handle: ReplicaHandle("card1".into()),
                    link_id: Some(ReplicaLinkId("uid:a".into())),
                    object: Some(ReplicaHash("0rig".into())),
                    level: ReplicaLevel::Full,
                    meta: Some(ReplicaMeta(r#"{"v":1}"#.into())),
                    sort_key: ReplicaSortKey::default(),
                    flags: ReplicaFlags::default(),
                    status: ReplicaStatus::Tombstone,
                    conflict_revision: None,
                    conflict_object: None,
                    base: Some(ReplicaBase {
                        flags: ReplicaFlags::default(),
                        revision: Some(String::from("v1")),
                        object: Some(ReplicaHash("0rig".into())),
                    }),
                    origin: None,
                }),
            ])
            .unwrap();
        store
    }

    fn sync_with(
        store: &mut PimdirSourceStore,
        remote: &mut MutableRemote,
        rights: ReplicaPushRights,
    ) -> ReplicaSyncReport {
        drive(
            store,
            remote,
            ReplicaSync::new(
                String::from("contacts"),
                sync_options(true, rights, ReplicaConflictPolicy::Manual),
            ),
        )
        .unwrap()
    }

    /// A side that may not delete holds the tombstone instead of undoing it.
    ///
    /// Both refusals (`push = false`, `item.delete = false`) run through one
    /// disposition, and for a hub-backed store it has to be `Keep`. Undoing the
    /// delete writes the placement back clean, which says the source still
    /// holds the member; the hub takes that as the item being alive and clears
    /// the deletion on every side, so removing an item once brings it back
    /// everywhere. The tombstone staying is what keeps the removal a removal
    /// until a run that may push delivers it.
    #[test]
    fn a_side_that_may_not_delete_keeps_the_tombstone() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_with_local_delete(dir.path());
        let mut remote = MutableRemote::at("card1", "v1");

        let rights = ReplicaPushRights {
            remove: false,
            ..ReplicaPushRights::all()
        };
        let report = sync_with(&mut store, &mut remote, rights);

        assert_eq!(report.pushed, 0, "the delete is not pushed");
        assert!(remote.pushed.is_empty(), "and nothing else is either");

        let placements = store
            .load(
                &ReplicaCollectionId("contacts".into()),
                &ReplicaLoadScope::All,
            )
            .unwrap()
            .placements;
        assert_eq!(placements.len(), 1);
        assert_eq!(
            placements[0].status,
            ReplicaStatus::Tombstone,
            "the removal is still a removal, not undone into a clean row",
        );
    }

    #[test]
    fn a_local_body_edit_is_pushed_conditionally_and_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_with_local_edit(dir.path(), "v1");
        let mut remote = MutableRemote::at("card1", "v1");

        let report = sync_with(&mut store, &mut remote, ReplicaPushRights::all());

        assert!(
            matches!(
                remote.pushed.as_slice(),
                [ReplicaChange { kind: ReplicaChangeKind::Update { if_match, .. }, .. }]
                    if if_match.as_deref() == Some("v1")
            ),
            "expected one If-Match update, got {:?}",
            remote.pushed
        );
        assert_eq!(report.conflicts, 0);
        assert_eq!(report.rejected, 0);

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
        let mut remote = MutableRemote::at("card1", "v9");

        let report = sync_with(&mut store, &mut remote, ReplicaPushRights::all());

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
        let placements = load_side(&store, "contacts").unwrap();
        assert_eq!(placements[0].status, ReplicaStatus::Dirty);
        assert_eq!(placements[0].object, Some(ReplicaHash("ed17".into())));
    }

    #[test]
    fn permissions_map_onto_io_replica_push_rights() {
        let perms = SourcePermissions {
            collection: crate::config::CollectionPermissions::default(),
            flag: crate::config::FlagSourcePermissions { update: false },
            item: crate::config::ItemSourcePermissions {
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

    /// A `Meta`-level placement with a base, the shape a side reports once its
    /// first reconcile has linked it.
    fn linked(handle: &str, link: &str, meta: &str) -> ReplicaPlacement {
        ReplicaPlacement {
            collection: ReplicaCollectionId("INBOX".into()),
            handle: ReplicaHandle(handle.into()),
            link_id: Some(ReplicaLinkId(link.into())),
            object: None,
            level: ReplicaLevel::Meta,
            meta: Some(ReplicaMeta(meta.into())),
            sort_key: ReplicaSortKey::default(),
            flags: ReplicaFlags::default(),
            status: ReplicaStatus::Clean,
            conflict_revision: None,
            conflict_object: None,
            base: Some(ReplicaBase {
                flags: ReplicaFlags::default(),
                revision: None,
                object: None,
            }),
            origin: None,
        }
    }

    /// Where the freeze this change reverses left one of two copies out of the
    /// store and warned about it, both copies are ordinary items now: the
    /// second carries the key the engine minted for it, projects a copy to the
    /// other side like any other item, and the run says nothing of its own
    /// about the pair.
    #[test]
    fn a_duplicated_identity_is_two_items_and_no_warning() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
        store.ensure_collection("INBOX", "message/rfc822").unwrap();

        store
            .write(vec![
                ReplicaWriteOp::UpsertPlacement(linked("145", "a@x", r#"{"v":1}"#)),
                ReplicaWriteOp::UpsertPlacement(linked("146", "dup:a@x#146", r#"{"v":1}"#)),
            ])
            .unwrap();

        let view = projection_view(&store, "INBOX", "left").unwrap();
        assert_eq!(view.len(), 2, "both copies are stored and projected");

        let mut report = SyncReport {
            account: "dup".into(),
            ..Default::default()
        };
        for placement in &view {
            for hunk in placement_hunks("left", "right", "INBOX", placement) {
                report.item.patch.push(PatchEntry::new(hunk, None));
            }
        }

        let text = report.to_string();
        assert!(!text.contains("Warnings"), "{text}");

        let json = serde_json::to_value(&report).unwrap();
        assert!(json.get("ambiguous").is_none(), "{json}");
        assert!(json.get("refused").is_none(), "{json}");
    }

    /// A target refusing the second copy is the one thing worth a line: the
    /// run wrote nothing for that item, and the line carries the `UID` and the
    /// collection the user repairs it by.
    #[test]
    fn a_refused_duplicate_is_named_with_its_uid() {
        let refused = vec![RefusedCreate {
            collection: String::from("agenda"),
            uid: String::from("event-1@google.com"),
        }];

        let mut report = SyncReport {
            account: "dup".into(),
            ..Default::default()
        };
        itemize_refused("right", refused, &mut report);

        assert_eq!(report.refused.len(), 1);
        assert_eq!(report.refused[0].side, "right");

        let text = report.to_string();
        assert!(text.contains("Warnings (1)"), "{text}");
        assert!(text.contains("agenda"), "{text}");
        assert!(text.contains("event-1@google.com"), "{text}");

        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["refused"][0]["side"], "right");
        assert_eq!(json["refused"][0]["collection"], "agenda");
        assert_eq!(json["refused"][0]["uid"], "event-1@google.com");
    }

    #[test]
    fn a_relayed_copy_is_itemized_so_the_run_never_reads_as_already_in_sync() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
        store.ensure_collection("INBOX", "message/rfc822").unwrap();

        store
            .write(vec![ReplicaWriteOp::UpsertPlacement(linked(
                "1",
                "mid:a@x",
                r#"{"v":1,"size":42}"#,
            ))])
            .unwrap();

        let targets = relay_targets(
            &store,
            Kind::Mail,
            "INBOX",
            ("left", false),
            ("right", true),
        )
        .unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].holding, "left");
        assert_eq!(targets[0].link, "mid:a@x");

        let target = &targets[0];
        let mut report = SyncReport {
            account: "relay".into(),
            ..Default::default()
        };
        report.item.patch.push(PatchEntry::new(
            ItemHunk::Copy {
                source_side: target.holding.clone(),
                target_side: String::from("right"),
                collection: "INBOX".into(),
                source_id: target.link.clone(),
                flags: target.flags.iter().cloned().collect(),
                content_key: content_key(&target.link),
            },
            None,
        ));

        let text = report.to_string();
        assert!(!text.contains("already in sync"), "{text}");
        assert!(text.contains("synchronized: 1 hunks"), "{text}");
        assert!(text.contains("from left to right"), "{text}");
    }

    /// A hub collection id carries its namespace and the wire name does not.
    /// Getting this backwards is how a mailbox and an address book both called
    /// `Default` would end up as one collection.
    #[test]
    fn a_hub_id_carries_its_namespace_and_the_display_name_does_not() {
        assert_eq!(hub_id("mail", "INBOX"), "mail/INBOX");
        assert_eq!(display_name("mail", "mail/INBOX"), "INBOX");

        // An IMAP hierarchy survives: only the first segment is the namespace.
        assert_eq!(hub_id("mail", "Archive/2026"), "mail/Archive/2026");
        assert_eq!(display_name("mail", "mail/Archive/2026"), "Archive/2026");

        // A name that merely starts with the namespace is not stripped.
        assert_eq!(display_name("mail", "mailbox/INBOX"), "mailbox/INBOX");

        // An id from another namespace is left whole rather than mangled.
        assert_eq!(display_name("cards", "mail/INBOX"), "mail/INBOX");
    }

    /// `--source` picks sources, there being no namespace to pick instead.
    #[test]
    fn narrowing_selects_the_named_sources() {
        let account: AccountConfig = toml::from_str(
            r#"
            sources.a.imap.server = "imaps://a.example.org:993"
            sources.b.imap.server = "imaps://b.example.org:993"
            sources.c.imap.server = "imaps://c.example.org:993"
            "#,
        )
        .unwrap();
        let mode = account.mode().unwrap();

        let picked = select_sources(&mode, &[String::from("a")]).unwrap();
        assert_eq!(picked, vec!["a"]);

        assert_eq!(select_sources(&mode, &[]).unwrap().len(), 3);

        let err = select_sources(&mode, &[String::from("nope")])
            .unwrap_err()
            .to_string();
        assert!(err.contains("no source named nope"), "got {err}");
    }

    /// The authority is the whole of `one-way`, and it is what stops a
    /// divergence from becoming a conflict nobody can resolve.
    #[test]
    fn the_authority_decides_the_conflict_and_the_push() {
        assert_eq!(
            Authority::Shared.conflict_policy(),
            ReplicaConflictPolicy::Manual,
        );
        assert!(
            Authority::Shared.writes_back(),
            "both sides write in a two-way mirror",
        );

        assert_eq!(
            Authority::Endpoint.conflict_policy(),
            ReplicaConflictPolicy::PreferRemote,
        );
        assert!(
            !Authority::Endpoint.writes_back(),
            "what the source holds is the truth, so nothing is written back to it",
        );

        assert_eq!(
            Authority::Store.conflict_policy(),
            ReplicaConflictPolicy::PreferLocal,
        );
        assert!(
            Authority::Store.writes_back(),
            "the target takes what the store pushes",
        );
    }

    /// A freshly probed item carries no link id, so it sits in the source's
    /// residual and never enters the hub. The pull plan has to read the side,
    /// not the projection, or a first sync of a kind whose identity lives in
    /// the body reports nothing at all.
    #[test]
    fn the_pull_plan_names_an_item_that_has_no_link_id_yet() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("dav");
        store
            .ensure_collection("dav/contacts", "text/vcard")
            .unwrap();

        store
            .write(vec![ReplicaWriteOp::UpsertPlacement(ReplicaPlacement {
                collection: ReplicaCollectionId("dav/contacts".into()),
                handle: ReplicaHandle("card-1.vcf".into()),
                link_id: None,
                object: None,
                level: ReplicaLevel::Probed,
                meta: None,
                sort_key: ReplicaSortKey::default(),
                flags: ReplicaFlags::default(),
                status: ReplicaStatus::Clean,
                conflict_revision: None,
                conflict_object: None,
                base: None,
                origin: None,
            })])
            .unwrap();

        assert!(
            projection_view(&store, "dav/contacts", "dav")
                .unwrap()
                .is_empty(),
            "the projection drops the residual, which is why it was the wrong read",
        );

        let mut report = SyncReport::default();
        itemize_fetches("dav/contacts", "contacts", &store, "dav", &mut report).unwrap();

        assert_eq!(report.item.patch.len(), 1);
        assert!(
            matches!(&report.item.patch[0].hunk, ItemHunk::Fetch { id, .. } if id == "card-1.vcf"),
            "named by its handle, the only name it has before its body arrives",
        );
    }

    /// A content change drops the stale object and leaves the level where it
    /// was, so a plan keyed on the level calls an item about to be re-fetched
    /// done.
    #[test]
    fn the_pull_plan_names_a_body_dropped_by_a_content_change() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("dav");
        store
            .ensure_collection("dav/contacts", "text/vcard")
            .unwrap();

        store
            .write(vec![ReplicaWriteOp::UpsertPlacement(ReplicaPlacement {
                collection: ReplicaCollectionId("dav/contacts".into()),
                handle: ReplicaHandle("card-1.vcf".into()),
                link_id: Some(ReplicaLinkId("uid:card-1".into())),
                object: None,
                level: ReplicaLevel::Full,
                meta: Some(ReplicaMeta(r#"{"v":1,"fn":"Jane"}"#.into())),
                sort_key: ReplicaSortKey::default(),
                flags: ReplicaFlags::default(),
                status: ReplicaStatus::Clean,
                conflict_revision: None,
                conflict_object: None,
                base: None,
                origin: None,
            })])
            .unwrap();

        let mut report = SyncReport::default();
        itemize_fetches("dav/contacts", "contacts", &store, "dav", &mut report).unwrap();

        assert_eq!(
            report.item.patch.len(),
            1,
            "Full with no object is a body to re-fetch, not a finished item",
        );
    }

    /// A calendar whose server implements no `sync-collection`: every
    /// enumeration is the whole member set, complete, under an empty
    /// checkpoint, and an identity is resolved only by downloading the body.
    /// The reported Posteo shape, with one `UID` under two hrefs.
    struct FullListingRemote {
        /// `handle -> (uid, body)`, in the order the listing returns them.
        items: Vec<(String, String, Vec<u8>)>,
        /// Every handle a body was fetched for, in order, so a re-run that
        /// downloads the same resource again is visible.
        fetched: Vec<String>,
    }

    impl ReplicaRemote for FullListingRemote {
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
                    .map(|(handle, _, _)| ReplicaRemoteItem {
                        handle: ReplicaHandle(handle.clone()),
                        flags: ReplicaFlags::from_iter([] as [String; 0]),
                        revision: Some(String::from("etag-1")),
                    })
                    .collect(),
                vanished: Vec::new(),
                complete: true,
                checkpoint: ReplicaCheckpoint(Vec::new()),
            })
        }

        fn fetch(
            &mut self,
            _collection: &ReplicaCollectionId,
            handles: Vec<ReplicaHandle>,
            _tier: ReplicaTier,
        ) -> Result<Vec<ReplicaFetchedItem>, Self::Error> {
            let mut items = Vec::new();
            for handle in handles {
                let Some((_, uid, body)) = self
                    .items
                    .iter()
                    .find(|(id, _, _)| *id == handle.0)
                    .cloned()
                else {
                    continue;
                };
                self.fetched.push(handle.0.clone());
                items.push(ReplicaFetchedItem {
                    handle,
                    link_id: ReplicaLinkId(uid),
                    meta: ReplicaMeta(format!(r#"{{"v":1,"size":{}}}"#, body.len())),
                    sort_key: ReplicaSortKey::default(),
                    body: Some(ReplicaFetchedBody::Inline {
                        hash: ReplicaHash(format!("{:016x}", digest(&body))),
                        bytes: body,
                    }),
                    revision: Some(String::from("etag-1")),
                });
            }
            Ok(items)
        }

        fn push(
            &mut self,
            _collection: &ReplicaCollectionId,
            _changes: Vec<ReplicaChange>,
        ) -> Result<Vec<ReplicaPushResult>, Self::Error> {
            anyhow::bail!("the listing remote is never pushed to")
        }
    }

    /// A content hash for a fake body, standing in for the store's own
    /// hasher: distinct bodies must name distinct objects or a test proves
    /// deduplication rather than storage.
    fn digest(body: &[u8]) -> u64 {
        body.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
    }

    /// One collection's spine as `collection_spine` runs it for a source with
    /// no counterpart: pull, report the bodies still to fetch, then raise the
    /// probed items to `Full`, which is where a DAV identity resolves.
    fn spine(
        store: &mut PimdirSourceStore,
        remote: &mut FullListingRemote,
        collection: &str,
    ) -> SyncReport {
        let mut report = SyncReport::default();
        drive(
            store,
            remote,
            ReplicaSync::new(
                collection.to_string(),
                sync_options(
                    false,
                    ReplicaPushRights::all(),
                    ReplicaConflictPolicy::Manual,
                ),
            ),
        )
        .unwrap();
        itemize_fetches(collection, "agenda", store, "caldav", &mut report).unwrap();

        let probed: Vec<ReplicaHandle> = load_side(store, collection)
            .unwrap()
            .into_iter()
            .filter(|p| p.level == ReplicaLevel::Probed)
            .map(|p| p.handle)
            .collect();
        if !probed.is_empty() {
            drive(
                store,
                remote,
                ReplicaUpgrade::new(collection.to_string(), probed, ReplicaTier::Full),
            )
            .unwrap();
        }
        report
    }

    /// The reported case, end to end: a calendar holding one `UID` under two
    /// hrefs mirrors as two items with two bodies, and the run after it says
    /// nothing at all.
    ///
    /// The second run is the bug the user saw. The frozen twin never got a
    /// row, so the pull plan read it as an unfetched body and named it on
    /// every run; the collection is listed in full every run here, exactly as
    /// that server lists it, so nothing but a row of its own could ever stop
    /// the line coming back.
    #[test]
    fn one_identity_under_two_hrefs_mirrors_as_two_items_and_settles() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("caldav");
        store
            .ensure_collection("caldav/agenda", "text/calendar")
            .unwrap();

        let uid = "event-1@google.com";
        let mut remote = FullListingRemote {
            items: vec![
                (
                    String::from("event-1%40google.com.ics"),
                    String::from(uid),
                    b"BEGIN:VCALENDAR:one".to_vec(),
                ),
                (
                    String::from("event-1%2540google.com.ics"),
                    String::from(uid),
                    b"BEGIN:VCALENDAR:two".to_vec(),
                ),
            ],
            fetched: Vec::new(),
        };

        let first = spine(&mut store, &mut remote, "caldav/agenda");
        assert_eq!(
            first.item.patch.len(),
            2,
            "both resources are bodies to fetch on the first run",
        );
        assert_eq!(remote.fetched.len(), 2, "each body is downloaded once");

        let mut placements = load_side(&store, "caldav/agenda").unwrap();
        placements.sort_by(|a, b| a.handle.0.cmp(&b.handle.0));
        assert_eq!(placements.len(), 2, "the twin has a row of its own");
        assert!(
            placements.iter().all(|p| p.object.is_some()),
            "each copy holds its own body",
        );
        assert_ne!(
            placements[0].object, placements[1].object,
            "two resources, two bodies",
        );

        let keys: Vec<String> = placements
            .iter()
            .map(|p| p.link_id.clone().unwrap().0)
            .collect();
        let bare = keys
            .iter()
            .position(|key| key == uid)
            .expect("one copy keeps the identity bare");
        let minted = 1 - bare;
        assert_eq!(
            keys[minted],
            format!("dup:{uid}#{}", placements[minted].handle.0),
            "the other is minted on the href it came from",
        );

        // The run the user actually complained about: the same full listing,
        // and nothing left to say about it.
        let second = spine(&mut store, &mut remote, "caldav/agenda");
        assert!(
            second.item.patch.is_empty(),
            "a settled collection reports nothing: {:?}",
            second
                .item
                .patch
                .iter()
                .map(|e| e.hunk.to_string())
                .collect::<Vec<_>>(),
        );
        assert!(second.refused.is_empty());
        assert_eq!(
            remote.fetched.len(),
            2,
            "no body is downloaded twice, so no blob is orphaned",
        );
    }
}
