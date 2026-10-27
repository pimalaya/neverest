//! # Sync orchestration
//!
//! Per-account, per-namespace and per-collection orchestration over the
//! io-pimdir engine.
//!
//! Each collection's two sides are the two sources of one shared collection in
//! a pimdir store, one [`PimdirSourceStore`] handle per side over the same
//! files. The driver runs the engine's per-side sync, an upgrade to resolve
//! link ids and a `Full` upgrade to hydrate a body about to be copied.
//!
//! Cross-side propagation of items, flags and deletions falls out of the shared
//! hub's project and absorb, so there is no hand-rolled cross-merge, and
//! syncing the sides in turn until quiescent converges them.
//!
//! Collection deletion is not propagated yet, only creation. The itemized
//! report lists the cross-side copies, flags and deletes a run plans, plus what
//! its opening pull-only round found the servers had already done.

use std::{
    cmp::Ordering as CmpOrdering,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    fmt, fs,
    io::Write,
    mem,
    path::{Path, PathBuf},
    process,
    sync::{
        Condvar, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Instant,
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use clap::ValueEnum;
use crossbeam_queue::SegQueue;
use io_pimdir::{
    change::PimdirWriteOp,
    client::{
        PimdirError, PimdirSourceStore, PimdirStore, blobs::PimdirBlobs, producer::PimdirProducer,
    },
    codec::PimdirAction,
    collection::{PimdirCollectionId, PimdirScope},
    hub::{PimdirBinding, PimdirSourceId},
    intent::{COLLECTION_CREATE, PimdirCollectionCreate},
    load::PimdirLoadScope,
    mutate::PimdirMutation,
    object::{PimdirHash, PimdirObject},
    placement::{
        PimdirBase, PimdirFlags, PimdirHandle, PimdirLevel, PimdirLinkId, PimdirPlacement,
        PimdirSortKey, PimdirStatus,
    },
    rekey::{PimdirRekey, PimdirRekeyReport},
    remote::{PimdirFetchedItem, PimdirListing, PimdirRemote, PimdirTier},
    summary::PimdirSummary,
    sync::{
        PimdirConflictPolicy, PimdirPushRights, PimdirSync, PimdirSyncEvent, PimdirSyncOptions,
        PimdirSyncReport,
    },
    upgrade::PimdirUpgrade,
};
use log::{debug, info, warn};
use pimalaya_cli::spinner::Spinner;

#[cfg(feature = "msgraph")]
use crate::msgraph::client::GraphKind;
use crate::{
    account::{Account, SourceAccount},
    client::{Client, Pool, WrittenItem},
    config::{
        AccountConfig, AccountMode, CollectionFilter, CollectionPermissions, SourceBackendConfig,
        SourceConfig, SourcePermissions,
    },
    item::{collection::Collection, flag::Flag},
    kind::{Kind, LinkId, merge::Merged},
    offline::{
        Paged, Pager, Rest, capability, create,
        invitation::{
            self, Failure as InvitationFailure, Invitation, InvitationIntent, Refusal, Target,
        },
        pipe,
        remote::{
            BATCH_SIZE, Bound, CachedFetchRemote, PimRemote, RefusedCreate, RejectedPush,
            body_bytes, hydrate_batch, resolve_kind, stored_item, wire_name,
        },
        run_verb, run_verb_paged, source_id,
        state::{LookedUpRoles, StoreState},
        storage::{HydrationSide, hydration_targets, load_side, projection_view},
        submit,
    },
    sync::{
        hunk::{CollectionHunk, ItemHunk},
        report::{
            CollectionCoverage, DrainedQueue, IntentEntry, ItemConflict, OpenRound,
            ParkedQueueAction, PatchEntry, PurgedItems, RefusedDuplicate, RejectedWrite,
            SourceDownload, SubmitEntry, SyncOutput, ThrottledSource, UnreachedEndpoint,
        },
    },
    throttle,
};

/// How many extra sync passes to run after the first.
///
/// Two or three converge a collection in practice, so the cap only guards
/// against a pathological loop.
const MAX_EXTRA_PASSES: usize = 4;

/// The remote name behind a hub collection id, for a report the user reads.
///
/// A report names a collection the way its server does, not the way the store
/// keys it: that is the name the user typed and the one their client shows.
fn display_name<'a>(namespace: &str, collection: &'a str) -> &'a str {
    wire_name(namespace, collection)
}

/// The hub collection id a remote collection name binds to in `namespace`.
///
/// Where the namespace and the name meet: a mailbox and an address book both
/// called `Default` land on different ids, and two providers cached side by
/// side never share one. The prefix is stripped again before any wire call.
fn hub_id(namespace: &str, name: &str) -> String {
    format!("{namespace}/{name}")
}

/// Resolves `<state base>/neverest/<account>/`, the default replica root.
pub fn replica_dir(account: &str) -> Result<PathBuf> {
    Ok(state_base()?.join("neverest").join(account))
}

/// The platform directory the default replica root sits under.
///
/// Only the base varies, the layout below it being one shape everywhere.
/// `dirs` reports a state directory on Linux and the BSDs alone, so a
/// platform without one takes where its own convention keeps app data.
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "ios")))]
fn state_base() -> Result<PathBuf> {
    dirs::state_dir().context("Cannot resolve the XDG state directory")
}

/// The platform directory the default replica root sits under.
///
/// `~/Library/Application Support` rather than `~/Library/Caches`: a dropped
/// store costs a full resync, and holds the only local copy when the account
/// retains bodies.
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn state_base() -> Result<PathBuf> {
    dirs::data_dir().context("Cannot resolve the Application Support directory")
}

/// The platform directory the default replica root sits under.
///
/// `%LOCALAPPDATA%` rather than the roaming `%APPDATA%`: the store holds
/// message bodies and a SQLite index, neither of which belongs on a profile
/// that syncs between machines.
#[cfg(target_os = "windows")]
fn state_base() -> Result<PathBuf> {
    dirs::data_local_dir().context("Cannot resolve the local application data directory")
}

/// The account's store directory: `store.root`, else [`replica_dir`].
pub fn store_dir(account: &str, config: &AccountConfig) -> Result<PathBuf> {
    match &config.store.root {
        Some(root) => Ok(root.clone()),
        None => replica_dir(account),
    }
}

/// The connections a Microsoft Graph source opens unless its `pool-size`
/// says otherwise.
///
/// Graph serves four requests at once a mailbox, the inner requests of a
/// `$batch` counting among them, so three leave room for another client of
/// the mailbox.
const GRAPH_CONNECTIONS: usize = 3;

/// How many connections a side may open, each one worker of the run.
///
/// IMAP takes `connections`, the account's or the run's. Microsoft Graph
/// takes its `pool-size`, else [`GRAPH_CONNECTIONS`], never more than
/// `connections`, so `--connections 1` serialises any source. Every other
/// HTTP backend keeps one.
fn connection_budget(config: &SourceConfig, connections: usize) -> usize {
    let connections = connections.max(1);
    match &config.backend {
        SourceBackendConfig::Imap(_) => connections,
        SourceBackendConfig::Msgraph(_)
        | SourceBackendConfig::MsgraphContacts(_)
        | SourceBackendConfig::MsgraphCalendar(_) => config
            .pool_size()
            .unwrap_or(GRAPH_CONNECTIONS)
            .clamp(1, connections),
        // NOTE: Gmail stays on one connection. Its batches pass one at a time
        // across the source (`GmailRun::enter`), and its pace and minute
        // budget are shared through the throttle, but a listing page or a
        // lone read on a second connection would run beside a batch, Gmail
        // binding per-user concurrency, and that was never measured: the
        // arm to widen once it is.
        SourceBackendConfig::Gmail(_) => 1,
        // NOTE: Google Calendar and People meter the account as a whole,
        // and a DAV server is no faster for being asked twice.
        _ => 1,
    }
}

/// Opens one source handle over the account's store.
///
/// Every collection it writes is grouped under `account` (pimdir SPEC §9.2),
/// so a store shared by two accounts tells whose is whose. A store an earlier
/// format wrote cannot be migrated, so the refusal names the `--reset` fix.
pub(crate) fn open_store(dir: &Path, source: &str, account: &str) -> Result<PimdirSourceStore> {
    match PimdirStore::open(dir) {
        Ok(store) => Ok(store.for_account(account).for_source(source)),
        Err(err @ (PimdirError::Version { .. } | PimdirError::Stale { .. })) => Err(
            anyhow::Error::new(err).context(format!(
                "The replica store predates this neverest; drop it with `neverest sync --reset -a {account}` and let it resync"
            )),
        ),
        Err(err) => Err(anyhow::Error::new(err).context(format!("Open {source} store"))),
    }
}

/// Marks the failure to open an endpoint's connections, which the run
/// reports as the endpoint unreached rather than as any failure of its sync.
#[derive(Debug)]
pub(crate) struct Unreached {
    /// The source or target that could not be opened.
    pub(crate) endpoint: String,
    /// What was being opened, the message this layer of the chain shows.
    what: String,
}

impl Unreached {
    fn new(endpoint: &str, what: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.to_owned(),
            what: what.into(),
        }
    }
}

impl fmt::Display for Unreached {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.what)
    }
}

impl std::error::Error for Unreached {}

/// The one kind a two-side account syncs, refusing a disagreeing pair.
///
/// The kind is never configured, falling out of each side's backend, so this is
/// the only enforcement point. It runs as soon as the media types are known and
/// before the first store write, so a mismatch fails with nothing half-written.
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

/// The pure half of [`check_kinds`]: two media types in, the agreed kind out.
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

/// Live per-collection progress: the spinner and its base label.
///
/// An inner phase such as body hydration or relay appends a percentage to the
/// `[2/7] Syncing INBOX` line.
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
/// This is the whole of `one-way`. Without it both sides are authoritative and
/// every divergence has to become a conflict; declaring an authority is what
/// removes the conflict rather than resolving it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Authority {
    /// Neither side wins: a divergence is a conflict. The two-way default.
    Shared,
    /// The endpoint wins and the store never writes back: the `sources` side.
    Endpoint,
    /// The store wins, overwriting the endpoint: the `targets` side.
    Store,
}

impl Authority {
    /// How a divergence between the endpoint and the store resolves.
    ///
    /// `remote` is the endpoint and `local` is the store. Neither authority
    /// records a conflict, which is the point: under `one-way` there is nothing
    /// for a user to resolve.
    fn conflict_policy(self) -> PimdirConflictPolicy {
        match self {
            Self::Shared => PimdirConflictPolicy::Manual,
            Self::Endpoint => PimdirConflictPolicy::PreferRemote,
            Self::Store => PimdirConflictPolicy::PreferLocal,
        }
    }

    /// Whether anything is written back; false for an authoritative endpoint.
    fn writes_back(self) -> bool {
        !matches!(self, Self::Endpoint)
    }
}

/// One connected endpoint plus its permissions and its authority.
struct SourceCtx {
    /// The configured name: its pimdir source id, and what every hunk carries.
    name: String,
    /// The hub namespace it binds into, stripped back off before any wire call.
    namespace: String,
    pool: Pool,
    perms: SourcePermissions,
    authority: Authority,
    /// The creates this endpoint refused, drained by [`itemize_refused`].
    refused: Vec<RefusedCreate>,
    /// The writes it would not take, drained by [`itemize_rejected`].
    ///
    /// That drain also un-counts the hunks the run had derived for them.
    rejected: Vec<RejectedPush>,
    /// The scope a mail collection of this endpoint lists (pimdir SYNC §5).
    scope: PimdirScope,
    /// Whether the run only plans, downloading no body.
    dry_run: bool,
}

impl SourceCtx {
    fn conflict_policy(&self) -> PimdirConflictPolicy {
        self.authority.conflict_policy()
    }

    /// A side pushes unless it forbids every mutation, or is authoritative.
    ///
    /// The two say the same thing, one through a permission table and one
    /// through the account's mode.
    fn writable(&self) -> bool {
        let rights = self.push_rights();
        self.authority.writes_back()
            && (rights.flags || rights.content || rights.add || rights.remove)
    }

    /// The side's configured permissions as io-pimdir's per-kind push rights.
    ///
    /// A mapping rather than a policy, the two vocabularies lining up one to
    /// one. The engine keeps a forbidden kind pending while the others still
    /// propagate, which is what makes a side read-only for some operations.
    fn push_rights(&self) -> PimdirPushRights {
        push_rights(&self.perms, self.pool.media_type())
    }
}

/// A side's permissions as io-pimdir's push rights for the kind it syncs.
///
/// Only mail carries flags on the wire: a flag on a card or a calendar item
/// is the store's alone (a `pimdir tag`, kept in the store only), so the
/// right is withheld and the engine keeps it local rather than pushing a
/// change the backend rejects on every run.
fn push_rights(perms: &SourcePermissions, media_type: &str) -> PimdirPushRights {
    PimdirPushRights {
        flags: perms.flag.update && media_type == "message/rfc822",
        content: perms.item.update,
        add: perms.item.create,
        remove: perms.item.delete,
    }
}

/// Runs the whole account sync and returns the report.
///
/// An account is one hub and one mode (see [`AccountConfig::mode`]). With no
/// target a source is reconciled against the store the app reads; with targets,
/// against each of them. Sources never see each other, their ids differing.
///
/// Under `declare_only`, each source lists every collection it holds and the
/// store records its kind, name and role, the filter aside, and no item
/// moves: what a client reads to offer the collections it could sync.
///
/// Under `intents_only`, the run drains the queue and performs what
/// frontends queued (collection creations, sends, calendar intents), opening
/// an endpoint only for an intent that needs it, then stops: it lists no
/// collection, pushes no other write and fetches nothing.
#[allow(clippy::too_many_arguments)]
pub fn run(
    account_name: impl Into<String>,
    account_config: &AccountConfig,
    collection_filter: Option<CollectionFilter>,
    scope: &PimdirScope,
    dry_run: bool,
    connections: usize,
    no_purge: bool,
    only_sources: &[String],
    accept_mode: bool,
    pacing: Pacing,
    declare_only: bool,
    intents_only: bool,
) -> Result<SyncOutput> {
    let account_name = account_name.into();
    let endpoints = account_config.endpoints()?;
    let mode = account_config.mode()?;
    let running = select_sources(&mode, only_sources)?;

    if declare_only && !mode.is_local() {
        bail!(
            "Declaring collections without syncing reads a local store only (an account with no target)"
        );
    }
    if intents_only && (declare_only || dry_run) {
        bail!("Performing the queued intents alone neither declares nor plans");
    }

    let real_dir = store_dir(&account_name, account_config)?;
    // NOTE: held for the whole run: dropping it removes the replica, so an
    // early return cannot leave one behind.
    let dry_replica = dry_run.then(|| DryRunReplica::new(&real_dir)).transpose()?;
    let work_dir = match &dry_replica {
        Some(replica) => replica.dir.clone(),
        None => real_dir.clone(),
    };
    fs::create_dir_all(&work_dir)
        .with_context(|| format!("Create replica dir {} error", work_dir.display()))?;

    let mut state = StoreState::load(&work_dir)?;

    let mut report = SyncOutput {
        account: account_name.clone(),
        dry_run,
        ..Default::default()
    };

    // NOTE: before any endpoint is opened.
    state.check_mode(&mode)?;

    // NOTE: before any credential is read, a declaration coming from the
    // configuration alone (pimdir STORAGE §15.6).
    if !dry_run {
        declare(&work_dir, &account_name, account_config, &mode)?;
    }

    // NOTE: once, before any credential is read or any endpoint opened, so a
    // run that cannot reach its servers still applies what frontends queued,
    // which they then read back. A declaration moves no item, so it leaves
    // the queue alone but for the collections to create, which it lists.
    if !declare_only {
        let first = running
            .first()
            .expect("a validated account has at least one source");
        let mut store = open_store(&work_dir, first, &account_name)?;
        drain_queues(&mut store, &mut report);
    }

    // NOTE: every credential the run needs, read once rather than per
    // connection.
    let account = Account::resolve_waiting(account_config, pacing.max_wait)?;

    for source_name in &running {
        let source = endpoints[source_name].clone();

        let outcome = if intents_only {
            run_intents(
                &account_name,
                account_config,
                &account,
                &mode,
                &endpoints,
                source_name,
                scope,
                &work_dir,
                &mut report,
            )
        } else if mode.is_local() {
            run_local(
                &account_name,
                account_config,
                &account,
                &mode,
                source_name,
                source,
                collection_filter.clone(),
                scope,
                &work_dir,
                dry_run,
                connections,
                pacing,
                declare_only,
                &mut state,
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
                scope,
                &work_dir,
                dry_run,
                connections,
                &mut report,
            )
        };

        // NOTE: sources share nothing but the store file, so one broken remote
        // is no reason to leave the others unsynced.
        if let Err(err) = outcome {
            note_source_failure(source_name, err, &mut report);
        }
    }

    // NOTE: once for the run, targets included: a source that gave up shares
    // its throttle with every connection opened from it.
    for (source, until) in account.throttled() {
        report.throttled.push(ThrottledSource {
            source,
            until: throttle::rfc3339(until),
        });
    }
    for (source, bytes) in account.downloaded() {
        report.downloaded.push(SourceDownload { source, bytes });
    }

    // NOTE: once for the run: the parked rows and the outstanding conflicts
    // belong to the store, and a per-source read would report each of them once
    // a source.
    match PimdirStore::open(&work_dir) {
        Ok(store) => {
            report_parked(&store, &mut report);
            count_conflicts(&store, &account_name, &mut report);
        }
        Err(err) => warn!("cannot open the store to read what it left behind: {err}"),
    }

    warn_conflicts(&report);

    if !dry_run {
        state.record_mode(&mode, accept_mode);

        // NOTE: again, so a calendar met for the first time this run takes
        // the intents its source performs there (pimdir STORAGE §15.6).
        if let Err(err) = declare(&work_dir, &account_name, account_config, &mode) {
            warn!("cannot declare what the sources hold after the run: {err:#}");
        }

        // NOTE: a run performing the queued intents alone purges nothing,
        // being one a frontend waits on.
        if !no_purge && !intents_only {
            let sweeper = running
                .first()
                .expect("a validated account has at least one source");
            let mut store = open_store(&work_dir, sweeper, &account_name)?;
            sweep_retained(account_config, &mut store, &mut report);
        }

        state.save(&work_dir)?;
    }

    crate::offline::prof::report();
    Ok(report)
}

/// Declares what every source of the account can do (pimdir STORAGE §15.6),
/// replacing what an earlier run or configuration declared.
pub(crate) fn declare(
    dir: &Path,
    account: &str,
    config: &AccountConfig,
    mode: &AccountMode,
) -> Result<()> {
    let sources = config.sources()?;
    let first = mode
        .sources
        .first()
        .expect("a validated account has at least one source");
    let mut store = open_store(dir, first, account)?;
    let collections = store
        .list_collections_by_account(Some(account))
        .context("List the collections the account syncs")?;

    for name in &mode.sources {
        let Some(source) = sources.get(name) else {
            continue;
        };
        // NOTE: a collection keyed in the source's namespace is on its
        // server, synced yet or only declared (`sync --declare-only`).
        let namespace = format!("{name}/");
        let mut held = Vec::new();
        for collection in &collections {
            let syncing = store
                .collection_sources(&collection.id)
                .with_context(|| format!("List the sources of {}", collection.id))?;
            if syncing.contains(name) || collection.id.starts_with(&namespace) {
                held.push(collection.id.clone());
            }
        }
        let capabilities = capability::declaration(source, mode.one_way, &held);
        store
            .declare(name, &capabilities)
            .with_context(|| format!("Declare the capabilities of source {name}"))?;
    }

    Ok(())
}

/// The sources this run touches: all of them, or those `--source` named.
///
/// Narrowing picks sources rather than namespaces, there being none: an account
/// is one mode, and a source reconciles on its own.
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

/// One source against every target the account names, each pairing on its own.
///
/// Both endpoints bind the source's namespace, which is what makes them meet.
/// Targets run in turn rather than together, no target influencing another
/// under `one-way`, so the source is enumerated once per target.
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
    scope: &PimdirScope,
    work_dir: &Path,
    dry_run: bool,
    connections: usize,
    report: &mut SyncOutput,
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
            scope,
            relay,
            work_dir,
            dry_run,
            connections,
            report,
        )?;
    }

    Ok(())
}

/// One source against one target, propagation falling out of the shared hub.
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
    scope: &PimdirScope,
    relay: bool,
    work_dir: &Path,
    dry_run: bool,
    connections: usize,
    report: &mut SyncOutput,
) -> Result<()> {
    let left_filter = left_config.collection().filter.clone();

    let mut left_store = open_store(work_dir, &left_name, account_name)?;
    let mut right_store = open_store(work_dir, &right_name, account_name)?;
    let blobs = left_store.blobs();

    // NOTE: declared, not derived: `retain` says whether the store is a
    // replica, and `relay` is only how a crossing gets there when it is not.
    let hydrate_full = mode.retain;

    let left_budget = connection_budget(&left_config, connections);
    let right_budget = connection_budget(&right_config, connections);

    // NOTE: under `one-way` the source is the truth and the target follows, so
    // neither side records a conflict; without it both are shared.
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
            .with_context(|| Unreached::new(&left_name, format!("Open source {left_name}")))?,
        refused: Vec::new(),
        rejected: Vec::new(),
        scope: scope.clone(),
        dry_run,
    };
    let mut right = SourceCtx {
        name: right_name.clone(),
        namespace: namespace.to_string(),
        perms: right_config.permissions(),
        authority: right_authority,
        pool: Pool::open(right_account, right_budget)
            .with_context(|| Unreached::new(&right_name, format!("Open target {right_name}")))?,
        refused: Vec::new(),
        rejected: Vec::new(),
        scope: scope.clone(),
        dry_run,
    };
    s.success("Opened endpoints");

    let kind = check_kinds(&mut left, &mut right)?;
    let media_type = kind.media_type();

    if !dry_run {
        drain_collection_creates(&mut [&mut left, &mut right], &mut left_store, report);
        drain_submits(
            account_config,
            account,
            &mut [SendSide::open(&mut left), SendSide::open(&mut right)],
            &mut left_store,
            &blobs,
            work_dir,
            report,
        );
        drain_invitations(&mut [&mut left, &mut right], &mut left_store, report);
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

    let held = held_by_both(
        &left_store,
        namespace,
        left_filtered.symmetric_difference(&right_filtered),
        &left_name,
        &right_name,
    )?;
    let collection_hunks = diff_collections(
        &left_filtered,
        &right_filtered,
        &held,
        (&left_name, &left.perms.collection),
        (&right_name, &right.perms.collection),
    );
    for hunk in collection_hunks {
        let error = if dry_run {
            None
        } else {
            apply_collection_hunk(&hunk, &mut left, &mut right).err()
        };
        // NOTE: gone from both endpoints, the collection leaves the store
        // too, so one created again under its name later reads as new
        // rather than as deleted on the other side.
        if !dry_run
            && error.is_none()
            && let CollectionHunk::Delete { collection, .. } = &hunk
        {
            let id = hub_id(namespace, collection);
            if let Err(err) = left_store.delete_collection(&id) {
                warn!("cannot drop {id} from the store: {err}");
            }
        }
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
        declare_name(
            &left_store,
            collection,
            name,
            &[&left_collections, &right_collections],
        );
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

/// A collection's spine result: its name and the bodies to hydrate.
type CollectionPlan = (String, Vec<HydrateTarget>);

/// A body to hydrate, with what orders its download.
#[derive(Clone, Debug)]
struct HydrateTarget {
    handle: PimdirHandle,
    /// The size its local mail summary carries, 0 when unknown.
    size: u64,
    /// Its date, the mail summary's RFC 3339 instant in UTC.
    date: Option<String>,
}

/// The order phase 2 downloads bodies in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum DownloadOrder {
    /// Largest first: the fastest whole run, the pool ending on small bodies.
    #[default]
    Largest,
    /// Newest first: recent mail is readable early on a large account.
    Newest,
}

impl DownloadOrder {
    /// Whether `a` downloads before `b`; a stable sort keeps ties in order.
    fn compare(self, a: &HydrateTarget, b: &HydrateTarget) -> CmpOrdering {
        match self {
            Self::Largest => b.size.cmp(&a.size),
            // NOTE: one fixed RFC 3339 shape in UTC, so the strings sort as
            // the instants do; `None` sorts below any date, hence last.
            Self::Newest => b.date.cmp(&a.date),
        }
    }
}

/// Which owed bodies a run downloads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Bodies {
    /// Every body the store still lacks, whichever run listed its item.
    #[default]
    All,
    /// Only the bodies of the items this run listed, the others staying
    /// owed for a later run: an arrival is readable at once, never behind
    /// the bodies an earlier run left.
    Listed,
}

/// How many of each collection's newest bodies `--prime` downloads by
/// default under `--download-order newest`: what a reader sees of a
/// mailbox before scrolling, the first chunk the Android client shows.
pub const DEFAULT_PRIME: usize = 50;

/// How a run orders and paces its work, never how much of it there is:
/// whatever the pacing, a run that ends has listed every page and
/// downloaded every body its collections owe ([`Bodies::Listed`] aside).
#[derive(Clone, Copy, Debug)]
pub struct Pacing {
    /// The order bodies download in.
    pub order: DownloadOrder,
    /// Under a positive count, every collection first lists its newest page
    /// and downloads that many of its newest bodies, before any collection
    /// lists a second page; 0 lists each collection whole in turn.
    pub prime: usize,
    /// Which owed bodies the run downloads.
    pub bodies: Bodies,
    /// The longest a source waits on a throttling provider before giving
    /// up for the run.
    pub max_wait: std::time::Duration,
}

impl Pacing {
    /// The pacing of a run, `prime` defaulting by the order: on under
    /// `newest`, the order a caller picks to show mail early, off under
    /// `largest`, the fastest whole run, which priming would only slow by
    /// a store read a collection.
    pub fn new(
        order: DownloadOrder,
        prime: Option<usize>,
        bodies: Bodies,
        max_wait: std::time::Duration,
    ) -> Self {
        let prime = prime.unwrap_or(match order {
            DownloadOrder::Newest => DEFAULT_PRIME,
            DownloadOrder::Largest => 0,
        });
        Self {
            order,
            prime,
            bodies,
            max_wait,
        }
    }
}

impl Default for Pacing {
    fn default() -> Self {
        Self::new(
            DownloadOrder::default(),
            None,
            Bodies::default(),
            throttle::MAX_WAIT,
        )
    }
}

/// Where a collection's role puts it in the run: what a reader opens
/// first, first. The inbox (or a default calendar or address book), then
/// sent mail and drafts, every other collection by name, the all-mail
/// archive that repeats them, then junk, and the trash last.
fn role_rank(role: Option<&str>) -> u8 {
    match role {
        Some("inbox" | "default") => 0,
        Some("sent") => 1,
        Some("drafts") => 2,
        Some("all") => 4,
        Some("junk") => 5,
        Some("trash") => 6,
        _ => 3,
    }
}

/// The collection ids `kept`, in the order the run takes them: by role
/// (see [`role_rank`]), then by name, ASCII case aside, then by id.
fn run_order(
    kept: &BTreeSet<String>,
    names: &BTreeMap<String, String>,
    roles: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut ordered: Vec<String> = kept.iter().cloned().collect();
    ordered.sort_by_cached_key(|id| {
        let role = role_rank(roles.get(id).map(String::as_str));
        let name = names.get(id).unwrap_or(id).to_ascii_lowercase();
        (role, name, id.clone())
    });
    ordered
}

/// The local, one-source sync, run as two account-wide phases.
///
/// Spine, then hydrate, each account-wide, so the connection pool
/// stays saturated end to end rather than idling at collection boundaries. The
/// store is the single local copy the app reads, so nothing crosses.
#[allow(clippy::too_many_arguments)]
fn run_local(
    account_name: &str,
    account_config: &AccountConfig,
    account: &Account,
    mode: &AccountMode,
    source_name: &str,
    source_config: SourceConfig,
    collection_filter: Option<CollectionFilter>,
    scope: &PimdirScope,
    work_dir: &Path,
    dry_run: bool,
    connections: usize,
    pacing: Pacing,
    declare_only: bool,
    state: &mut StoreState,
    report: &mut SyncOutput,
) -> Result<()> {
    let source = source_name.to_string();
    let source_filter = source_config.collection().filter.clone();

    // NOTE: a declaration lists and fetches nothing, one connection is enough.
    let workers = if declare_only {
        1
    } else {
        connection_budget(&source_config, connections)
    };
    let s = Spinner::start(format!("Opening connections to {source_name}…"));
    // NOTE: with no target the store is the destination: `one-way` discards
    // what was staged locally, leaving it off merges the two.
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
        scope,
        dry_run,
    )?;
    let mut stores: Vec<PimdirSourceStore> = (0..workers)
        .map(|_| open_store(work_dir, &source, account_name))
        .collect::<Result<_>>()?;
    let blobs = stores[0].blobs();
    s.success(format!(
        "Opened {} connection(s) to {source_name}",
        ctxs.len()
    ));

    let raw = ctxs[0].pool.primary().media_type();
    let kind = Kind::from_media_type(raw)
        .with_context(|| format!("This build cannot sync items of type {raw}"))?;
    let media_type = kind.media_type();

    if !dry_run {
        let (first, _) = ctxs.split_first_mut().expect("at least one connection");
        drain_collection_creates(&mut [&mut *first], &mut stores[0], report);
        if !declare_only {
            drain_submits(
                account_config,
                account,
                &mut [SendSide::open(&mut *first)],
                &mut stores[0],
                &blobs,
                work_dir,
                report,
            );
            drain_invitations(&mut [first], &mut stores[0], report);
        }
    }

    let s = Spinner::start(format!("Listing collections on {source_name}…"));
    let listed = ctxs[0]
        .pool
        .primary()
        .list_collections(false)
        .context("List collections error")?;
    let collections: BTreeMap<String, String> = listed
        .iter()
        .map(|collection| (collection.id.clone(), collection.name.clone()))
        .collect();
    s.success(format!(
        "Listed {} collection(s) on {source_name}",
        collections.len()
    ));

    let filter = if declare_only {
        CollectionFilter::All
    } else {
        collection_filter.unwrap_or(source_filter)
    };
    let kept = filter_collections(&collections, &filter);

    let stated = stated_roles(
        || ctxs[0].pool.primary().lookup_roles(),
        source_name,
        &listed,
        state,
    );
    // NOTE: a failed lookup leaves the recorded roles alone, and the run
    // orders by those the listing states.
    let roles = stated.clone().unwrap_or_else(|| {
        listed
            .iter()
            .filter_map(|c| Some((c.id.clone(), c.role.clone()?)))
            .collect()
    });
    let kept = run_order(&kept, &collections, &roles);
    let filtered: Vec<String> = kept.iter().map(|id| hub_id(source_name, id)).collect();

    for (collection, id) in filtered.iter().zip(&kept) {
        stores[0]
            .ensure_collection(collection, media_type)
            .with_context(|| format!("Declare kind for {collection}"))?;
        declare_name(&stores[0], collection, id, &[&collections]);
        if stated.is_some() {
            declare_role(&stores[0], collection, roles.get(id).map(String::as_str));
        }
    }

    if declare_only {
        return Ok(());
    }

    // NOTE: a dry run lists every page to plan the run, and downloads none.
    let pacing = Pacing {
        prime: if dry_run { 0 } else { pacing.prime },
        ..pacing
    };
    let listed_log = (pacing.bodies == Bodies::Listed).then(Listed::default);
    let shared = ctxs[0].pool.primary().global_handles();
    let gone = GoneBodies::default();

    let spined = phase1_spine(
        source_name,
        &filtered,
        &mut ctxs,
        &mut stores,
        &blobs,
        work_dir,
        dry_run,
        &pacing,
        listed_log.as_ref(),
        shared,
        &gone,
        report,
    )?;

    if dry_run {
        return Ok(());
    }

    // NOTE: read once phase 1 is over, so a body a landed page's hydration
    // kept is not downloaded again, nor one it found gone.
    let plans = hydrate_plans(&stores[0], source_name, &spined, listed_log.as_ref(), &gone)?;
    phase2_hydrate(
        source_name,
        plans,
        &pacing,
        &mut ctxs,
        &mut stores,
        &blobs,
        shared.then_some(&filtered[..]),
        &gone,
    )
}

/// Performs what frontends queued for one source and, in a paired account,
/// its targets, syncing nothing: the collection creations, the sends and
/// the calendar intents, as a full run does before it lists anything.
///
/// An endpoint is opened only when one of these needs its session: an
/// intent it performs on its server, or a send through its native channel
/// (Graph). A send through an `smtp` table opens that channel alone, so a
/// message leaves while the source's own server is down. An endpoint that
/// does not open is reported unreached and the others still perform theirs.
#[allow(clippy::too_many_arguments)]
fn run_intents(
    account_name: &str,
    account_config: &AccountConfig,
    account: &Account,
    mode: &AccountMode,
    endpoints: &HashMap<String, SourceConfig>,
    source_name: &str,
    scope: &PimdirScope,
    work_dir: &Path,
    report: &mut SyncOutput,
) -> Result<()> {
    // NOTE: a target binds its source's namespace, as in a paired sync.
    let names: Vec<&str> = std::iter::once(source_name)
        .chain(mode.targets.iter().map(String::as_str))
        .collect();
    let mut store = open_store(work_dir, source_name, account_name)?;
    let blobs = store.blobs();
    let wanted = wanted_sides(&store, account_config, &names, source_name);

    let mut opened: Vec<SourceCtx> = Vec::new();
    for &name in names.iter().filter(|name| wanted.contains(**name)) {
        let authority = match (mode.one_way, name == source_name) {
            (false, _) => Authority::Shared,
            (true, true) => Authority::Endpoint,
            (true, false) => Authority::Store,
        };
        let ctxs = account.get(name).and_then(|side| {
            open_source_contexts(
                name,
                source_name,
                &endpoints[name],
                side,
                authority,
                1,
                scope,
                false,
            )
        });
        match ctxs {
            Ok(ctxs) => opened.extend(ctxs),
            Err(err) => note_source_failure(name, err, report),
        }
    }

    drain_collection_creates(
        &mut opened.iter_mut().collect::<Vec<_>>(),
        &mut store,
        report,
    );

    // NOTE: `opened` keeps the order of `names`, so one walk pairs them.
    let mut open = opened.iter_mut().peekable();
    let mut sides: Vec<SendSide<'_>> = Vec::with_capacity(names.len());
    for &name in &names {
        match open.next_if(|ctx| ctx.name == name) {
            Some(ctx) => sides.push(SendSide::open(ctx)),
            None => sides.push(SendSide::unopened(name)),
        }
    }
    drain_submits(
        account_config,
        account,
        &mut sides,
        &mut store,
        &blobs,
        work_dir,
        report,
    );
    drop(sides);

    drain_invitations(
        &mut opened.iter_mut().collect::<Vec<_>>(),
        &mut store,
        report,
    );
    Ok(())
}

/// The endpoints among `names` whose session a queued intent needs, the
/// drains picking their performer as they do.
///
/// A collection creation or a calendar intent needs the endpoint it names,
/// or, naming none (a producer predating pimdir STORAGE §15.6), the first
/// endpoint of its namespace; an unnamed calendar intent may need any of
/// them, the one binding its item being known only once they are open. A
/// send needs the sending endpoint only when it sends natively.
fn wanted_sides(
    store: &PimdirStore,
    account_config: &AccountConfig,
    names: &[&str],
    namespace: &str,
) -> BTreeSet<String> {
    let prefix = format!("{namespace}/");
    let mut wanted = BTreeSet::new();
    let mut want = |named: Option<String>, collection: &str, any: bool| match named {
        Some(name) if names.contains(&name.as_str()) => {
            wanted.insert(name);
        }
        Some(_) => {}
        None if collection.starts_with(&prefix) && any => {
            wanted.extend(names.iter().map(|name| name.to_string()));
        }
        None if collection.starts_with(&prefix) => {
            wanted.insert(names[0].to_owned());
        }
        None => {}
    };

    match create::pending(store) {
        Ok(intents) => {
            for intent in intents {
                want(intent.source(), &intent.collection, false);
            }
        }
        Err(err) => warn!("cannot read the queued collection intents: {err:#}"),
    }
    match invitation::pending(store) {
        Ok(intents) => {
            for intent in intents {
                want(intent.source(), &intent.collection, true);
            }
        }
        Err(err) => warn!("cannot read the queued calendar intents: {err:#}"),
    }

    let sides: Vec<SendSide<'_>> = names.iter().map(|name| SendSide::unopened(name)).collect();
    let sender = sending_source(account_config, &sides);
    let native = sender.as_ref().is_some_and(|sender| {
        account_config.sources().is_ok_and(|sources| {
            sources
                .get(sender)
                .is_some_and(|source| source.smtp.is_none() && source.sends_natively())
        })
    });
    if let Some(sender) = sender.filter(|_| native) {
        let queued = submit::pending(store).is_ok_and(|intents| {
            intents
                .iter()
                .any(|intent| intent.source().is_none_or(|source| source == sender))
        });
        if queued {
            wanted.insert(sender);
        }
    }

    wanted
}

/// Records an endpoint the run could not sync or open: a failed `*` scan,
/// and the endpoint unreached when the failure says so.
fn note_source_failure(source_name: &str, err: anyhow::Error, report: &mut SyncOutput) {
    warn!("source {source_name} sync error: {err:#}");
    if let Some(unreached) = err.downcast_ref::<Unreached>() {
        report.unreached.push(UnreachedEndpoint {
            endpoint: unreached.endpoint.clone(),
            error: format!("{err:#}"),
        });
    }
    report.collection.patch.push(PatchEntry::new(
        CollectionHunk::Scan {
            side: source_name.to_owned(),
            collection: String::from("*"),
        },
        Some(err),
    ));
}

/// Opens `count` single-connection [`SourceCtx`]s with overlapping handshakes.
#[allow(clippy::too_many_arguments)]
fn open_source_contexts(
    name: &str,
    namespace: &str,
    config: &SourceConfig,
    account: SourceAccount,
    authority: Authority,
    count: usize,
    scope: &PimdirScope,
    dry_run: bool,
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
            pool: pool.with_context(|| Unreached::new(name, "Open connection"))?,
            refused: Vec::new(),
            rejected: Vec::new(),
            scope: scope.clone(),
            dry_run,
        });
    }
    Ok(ctxs)
}

/// Phase 1, spining each collection in parallel over its own connection.
///
/// Reads run concurrently through WAL, writes serialise on the store's
/// single-writer lock. Each collection's report is absorbed whole, arms and
/// all: dropping one leaves the run naming what it touched, never what it left.
///
/// The collections are taken in the order of `filtered` (see [`run_order`]).
/// Under `--prime`, every collection first lists up to its newest page and
/// its newest bodies download right after, before any collection lists a
/// second page; the collections paused then are listed to their end once
/// that first pass is over (see [`Work`]).
///
/// A connection with no collection left to spine downloads the bodies of the
/// pages the others have landed, so a large collection's first bodies are
/// readable while its later pages still list. Returns the collections
/// spined to their end, in the run's order, whose remaining bodies phase 2
/// downloads.
#[allow(clippy::too_many_arguments)]
fn phase1_spine(
    source: &str,
    filtered: &[String],
    ctxs: &mut [SourceCtx],
    stores: &mut [PimdirSourceStore],
    blobs: &PimdirBlobs,
    store_dir: &Path,
    dry_run: bool,
    pacing: &Pacing,
    listed: Option<&Listed>,
    shared: bool,
    gone: &GoneBodies,
    report: &mut SyncOutput,
) -> Result<Vec<String>> {
    let total = filtered.len();
    let prime = pacing.prime;
    let work = Work::new(pacing.order, total, prime > 0);
    let spined: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    let carried: Mutex<HashMap<usize, HashSet<String>>> = Mutex::new(HashMap::new());
    let merged: Mutex<SyncOutput> = Mutex::new(SyncOutput::default());
    let scanned = AtomicUsize::new(0);
    let pushing: Mutex<()> = Mutex::new(());
    let s = Spinner::start(format!("Scanning {source} (0/{total})"));

    // NOTE: one connection has none to spare: its bodies wait for phase 2,
    // which orders them across the whole account, but for the primed ones.
    let feeding = !dry_run && ctxs.len() > 1;
    let gates: HashMap<&str, Mutex<()>> = filtered
        .iter()
        .map(|collection| (collection.as_str(), Mutex::new(())))
        .collect();
    let applying: Mutex<()> = Mutex::new(());
    let downloaded = AtomicUsize::new(0);
    let kind = ctxs
        .first_mut()
        .map(|ctx| resolve_kind(&mut ctx.pool))
        .unwrap_or(Kind::Mail);
    let landing = Landing {
        kind,
        blobs,
        gates: Some(&gates),
        applying: &applying,
        shared: shared.then_some(filtered),
        gone,
        on_body: None,
    };

    let work_ref = &work;
    let spined_ref = &spined;
    let carried_ref = &carried;
    let merged_ref = &merged;
    let scanned_ref = &scanned;
    let s_ref = &s;
    let pushing_ref = &pushing;
    let gates_ref = &gates;
    let landing_ref = &landing;
    let downloaded_ref = &downloaded;

    thread::scope(|scope| {
        for (ctx, store) in ctxs.iter_mut().zip(stores.iter_mut()) {
            scope.spawn(move || {
                while let Some(job) = work_ref.next() {
                    let (index, pass) = match job {
                        Job::Spine { index, pass } => (index, pass),
                        Job::Hydrate { batch, primed } => {
                            match landing_ref.land(ctx, store, &batch) {
                                Ok(fetched) => {
                                    downloaded_ref.fetch_add(fetched, Ordering::Relaxed);
                                }
                                // NOTE: left bodiless, so phase 2 downloads it
                                // again and fails the run if it fails there too.
                                Err(err) => {
                                    warn!("{} body download error: {err:#}", batch.collection)
                                }
                            }
                            work_ref.downloaded(primed);
                            continue;
                        }
                    };

                    let collection = &filtered[index];
                    let owed = carried_ref
                        .lock()
                        .unwrap()
                        .remove(&index)
                        .unwrap_or_default();
                    let mut pager = PageFeed {
                        // NOTE: the first pass queues its bodies once it ends,
                        // the newest first of all.
                        feed: (feeding && pass != Pass::First).then_some(work_ref),
                        rank: index,
                        // NOTE: what a resumed round owes, the first pass
                        // queued already.
                        resumed: pass == Pass::Whole,
                        pause: pass == Pass::First,
                        listed,
                        ..PageFeed::hold(&gates_ref[collection.as_str()])
                    };
                    let spine = collection_spine(
                        collection,
                        ctx,
                        store,
                        blobs,
                        store_dir,
                        pushing_ref,
                        dry_run,
                        &mut pager,
                        owed,
                    );
                    drop(pager);
                    let paused = match spine {
                        Ok(Spined {
                            report,
                            paused,
                            owed,
                        }) => {
                            merged_ref.lock().unwrap().absorb(report);
                            if paused {
                                carried_ref.lock().unwrap().insert(index, owed);
                            } else {
                                spined_ref.lock().unwrap().push(index);
                            }
                            paused
                        }
                        Err(err) => {
                            warn!("{collection} scan error: {err:#}");
                            let display = display_name(&ctx.namespace, collection).to_string();
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
                            // NOTE: a round the failure interrupted stays open,
                            // which the next run resumes.
                            report_coverage(
                                store,
                                collection,
                                ctx,
                                &mut merged_ref.lock().unwrap(),
                            );
                            false
                        }
                    };
                    if pass == Pass::First {
                        prime_bodies(work_ref, store, collection, index, prime, feeding, listed);
                    }
                    work_ref.spined(pass, index, paused);
                    if !paused {
                        let n = scanned_ref.fetch_add(1, Ordering::Relaxed) + 1;
                        s_ref.set_message(format!("Scanning {source} ({n}/{total})"));
                    }
                }
            });
        }
    });

    let mut spined = spined.into_inner().unwrap();
    spined.sort_unstable();
    report.absorb(merged.into_inner().unwrap());
    match downloaded.into_inner() {
        0 => s.success(format!("Scanned {total} collection(s) on {source}")),
        n => s.success(format!(
            "Scanned {total} collection(s) on {source}, downloaded {n} item(s)"
        )),
    }
    Ok(spined
        .into_iter()
        .map(|index| filtered[index].clone())
        .collect())
}

/// Queues the bodies a collection owes once the first pass over it ended:
/// its `prime` newest ahead of any other, then, connections to spare, the
/// rest beside the pages' bodies. Whether its round paused or closed, the
/// store holds what the run listed so far.
fn prime_bodies(
    work: &Work,
    store: &PimdirSourceStore,
    collection: &str,
    rank: usize,
    prime: usize,
    feeding: bool,
    listed: Option<&Listed>,
) {
    let id = PimdirCollectionId(collection.to_string());
    let mut owed = match store.load(&id, &PimdirLoadScope::All) {
        Ok(loaded) => bodiless(loaded.placements),
        // NOTE: what is not queued waits for phase 2.
        Err(err) => {
            warn!("{collection} load error: {err}");
            return;
        }
    };
    if let Some(listed) = listed {
        owed = listed.keep(collection, owed);
    }
    // NOTE: primed by date whatever the download order, the newest being
    // what a reader opens first; the order still ranks the batches.
    owed.sort_by(|a, b| DownloadOrder::Newest.compare(a, b));
    let rest = owed.split_off(prime.min(owed.len()));
    work.feed(collection, rank, owed, true);
    if feeding {
        work.feed(collection, rank, rest, false);
    }
}

/// The handles each collection's listing landed this run, kept under
/// [`Bodies::Listed`], whose run downloads those bodies alone.
#[derive(Default)]
struct Listed(Mutex<HashMap<String, HashSet<String>>>);

impl Listed {
    fn record(&self, collection: &str, landed: &[PimdirHandle]) {
        if landed.is_empty() {
            return;
        }
        let mut listed = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        listed
            .entry(collection.to_string())
            .or_default()
            .extend(landed.iter().map(|handle| handle.0.clone()));
    }

    /// The targets of `collection` this run listed.
    fn keep(&self, collection: &str, targets: Vec<HydrateTarget>) -> Vec<HydrateTarget> {
        let listed = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(handles) = listed.get(collection) else {
            return Vec::new();
        };
        targets
            .into_iter()
            .filter(|target| handles.contains(&target.handle.0))
            .collect()
    }
}

/// Which pass a phase-1 spine runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pass {
    /// Priming off: the collection listed to its end.
    Whole,
    /// Priming: the collection listed up to its first page, a round left
    /// open there, a delta to its end.
    First,
    /// Priming over: a collection the first pass paused, listed to its end.
    Fill,
}

/// What a phase-1 connection does next.
#[derive(Debug, PartialEq)]
enum Job {
    /// Spine the collection at `index` of the run's order.
    Spine { index: usize, pass: Pass },
    /// Download a batch, `primed` when it holds a collection's newest.
    Hydrate { batch: HydrateBatch, primed: bool },
}

/// What a queued batch is ranked by: its first body, then its collection's
/// place in the run.
#[derive(Debug)]
struct BatchKey {
    first: HydrateTarget,
    rank: usize,
}

/// Phase 1's work, handed to the connections in turn.
///
/// The collections to spine and the bodies their landed pages owe, waiting
/// for a free connection. A connection takes, in this order: a batch of
/// primed bodies; a collection the first pass has not reached; once that
/// pass and its primed bodies are over, a collection it paused, to fill;
/// else a batch of other bodies, the first in the download order across
/// the account, ties going to the collection the run takes first. It
/// waits while a spine or a primed download under way may queue more.
///
/// Only batches are held: every connection fetches one at a time into the
/// blob store, so the memory in flight is the budget times a batch's
/// handles, whatever the account holds.
struct Work {
    order: DownloadOrder,
    state: Mutex<WorkState>,
    ready: Condvar,
}

struct WorkState {
    /// The collections the first pass has not taken, in the run's order.
    first: VecDeque<usize>,
    /// The pass they are taken in: [`Pass::First`] under priming, else
    /// [`Pass::Whole`].
    pass: Pass,
    /// The collections the first pass paused, waiting for it to end.
    fill: BTreeSet<usize>,
    /// The spines under way.
    spining: usize,
    /// The spines under way taken from `first`.
    priming: usize,
    /// The batches of primed bodies, and how many are downloading.
    primed: Vec<(BatchKey, HydrateBatch)>,
    downloading: usize,
    /// The batches of every other body.
    batches: Vec<(BatchKey, HydrateBatch)>,
}

impl Work {
    /// The work of `collections` spines, primed or listed whole.
    fn new(order: DownloadOrder, collections: usize, prime: bool) -> Self {
        Self {
            order,
            state: Mutex::new(WorkState {
                first: (0..collections).collect(),
                pass: if prime { Pass::First } else { Pass::Whole },
                fill: BTreeSet::new(),
                spining: 0,
                priming: 0,
                primed: Vec::new(),
                downloading: 0,
                batches: Vec::new(),
            }),
            ready: Condvar::new(),
        }
    }

    /// Queues `collection`'s bodiless targets, in batches of [`BATCH_SIZE`]
    /// sorted in the download order.
    fn feed(&self, collection: &str, rank: usize, mut targets: Vec<HydrateTarget>, primed: bool) {
        if targets.is_empty() {
            return;
        }
        targets.sort_by(|a, b| self.order.compare(a, b));
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let queue = if primed {
            &mut state.primed
        } else {
            &mut state.batches
        };
        for chunk in targets.chunks(BATCH_SIZE) {
            let key = BatchKey {
                first: chunk[0].clone(),
                rank,
            };
            queue.push((key, HydrateBatch::new(collection, chunk)));
        }
        drop(state);
        self.ready.notify_all();
    }

    /// A spine ended, whether it succeeded or not, `paused` when the first
    /// pass left its round open.
    fn spined(&self, pass: Pass, index: usize, paused: bool) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.spining = state.spining.saturating_sub(1);
        if pass != Pass::Fill {
            state.priming = state.priming.saturating_sub(1);
        }
        if paused {
            state.fill.insert(index);
        }
        drop(state);
        self.ready.notify_all();
    }

    /// A batch was downloaded, applied or not.
    fn downloaded(&self, primed: bool) {
        if !primed {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.downloading = state.downloading.saturating_sub(1);
        drop(state);
        self.ready.notify_all();
    }

    /// The next job, waiting while one may still come; `None` once every
    /// spine and primed download has ended and nothing is queued.
    fn next(&self) -> Option<Job> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(batch) = self.take(&mut state.primed) {
                state.downloading += 1;
                return Some(Job::Hydrate {
                    batch,
                    primed: true,
                });
            }
            if let Some(index) = state.first.pop_front() {
                state.spining += 1;
                state.priming += 1;
                let pass = state.pass;
                return Some(Job::Spine { index, pass });
            }
            // NOTE: the first pass is over once its last spine and its
            // last primed body are, so no collection lists a second page
            // before every one has listed its first.
            if state.priming == 0
                && state.downloading == 0
                && let Some(index) = state.fill.pop_first()
            {
                state.spining += 1;
                return Some(Job::Spine {
                    index,
                    pass: Pass::Fill,
                });
            }
            if let Some(batch) = self.take(&mut state.batches) {
                return Some(Job::Hydrate {
                    batch,
                    primed: false,
                });
            }
            if state.spining == 0 && state.downloading == 0 {
                return None;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Takes the batch of `queue` that comes first.
    fn take(&self, queue: &mut Vec<(BatchKey, HydrateBatch)>) -> Option<HydrateBatch> {
        let first = queue
            .iter()
            .enumerate()
            .min_by(|(_, (a, _)), (_, (b, _))| {
                self.order
                    .compare(&a.first, &b.first)
                    .then(a.rank.cmp(&b.rank))
            })
            .map(|(index, _)| index)?;
        Some(queue.remove(first).1)
    }
}

/// The [`Pager`] of a collection's spine in phase 1.
///
/// It holds the collection's gate for the whole spine but the listing's
/// waits on the server, so a body is applied to the collection only where
/// the sync is at rest, never between a page's load and its write, which
/// would write back the placement without the body. Each landed page's
/// bodiless members go to the feed, and, when `resumed`, the bodiless
/// members of a resumed round's earlier pages before its first page lists.
/// Under `pause`, the listing stops before the second page of a round.
struct PageFeed<'a> {
    gate: &'a Mutex<()>,
    held: Option<MutexGuard<'a, ()>>,
    feed: Option<&'a Work>,
    /// The collection's place in the run, which breaks the feed's ties.
    rank: usize,
    resumed: bool,
    pause: bool,
    listed: Option<&'a Listed>,
}

impl<'a> PageFeed<'a> {
    /// A pager feeding nothing, its gate held.
    fn hold(gate: &'a Mutex<()>) -> Self {
        Self {
            gate,
            held: Some(gate.lock().unwrap_or_else(PoisonError::into_inner)),
            feed: None,
            rank: 0,
            resumed: true,
            pause: false,
            listed: None,
        }
    }
}

impl Pager for PageFeed<'_> {
    fn at_rest(
        &mut self,
        store: &mut PimdirSourceStore,
        collection: &PimdirCollectionId,
        landed: Vec<PimdirHandle>,
        resumed: bool,
        next: Option<&PimdirListing>,
    ) -> Rest {
        if let Some(listed) = self.listed {
            listed.record(collection.as_str(), &landed);
        }
        // NOTE: a round asking for a page past its first, whether this run
        // listed that one or an earlier run did.
        if self.pause
            && matches!(
                next,
                Some(PimdirListing::Round {
                    cursor: Some(_),
                    ..
                })
            )
        {
            return Rest::Pause;
        }
        let Some(feed) = self.feed else {
            return Rest::Go;
        };
        let scope = match (resumed && self.resumed, landed.is_empty()) {
            (true, _) => PimdirLoadScope::All,
            (false, true) => return Rest::Go,
            (false, false) => PimdirLoadScope::Handles(landed),
        };
        match store.load(collection, &scope) {
            Ok(loaded) => {
                let mut owed = bodiless(loaded.placements);
                if let Some(listed) = self.listed {
                    owed = listed.keep(collection.as_str(), owed);
                }
                feed.feed(collection.as_str(), self.rank, owed, false);
            }
            // NOTE: what is not fed waits for phase 2.
            Err(err) => warn!("{} page load error: {err}", collection.as_str()),
        }
        Rest::Go
    }

    fn listing(&mut self) {
        self.held = None;
    }

    fn listed(&mut self) {
        self.held = Some(self.gate.lock().unwrap_or_else(PoisonError::into_inner));
    }
}

/// The placements still owed a body, with what orders their download.
fn bodiless(placements: impl IntoIterator<Item = PimdirPlacement>) -> Vec<HydrateTarget> {
    placements
        .into_iter()
        .filter(|placement| {
            placement.status != PimdirStatus::Tombstone && placement.object.is_none()
        })
        .map(|placement| HydrateTarget {
            size: summary_size(&placement.summary).unwrap_or(0) as u64,
            date: summary_date(&placement.summary),
            handle: placement.handle,
        })
        .collect()
}

/// Each spined collection's bodies still to download, for phase 2, only
/// those the run listed under [`Bodies::Listed`], none the run found gone.
fn hydrate_plans(
    store: &PimdirStore,
    source: &str,
    collections: &[String],
    listed: Option<&Listed>,
    gone: &GoneBodies,
) -> Result<Vec<CollectionPlan>> {
    collections
        .iter()
        .map(|collection| {
            let view = projection_view(store, collection, source)
                .with_context(|| format!("Project {source} {collection}"))?;
            let owed = gone.keep(collection, bodiless(view));
            let owed = match listed {
                Some(listed) => listed.keep(collection, owed),
                None => owed,
            };
            Ok((collection.clone(), owed))
        })
        .collect()
}

/// A collection's spine: its report, and whether the pager paused it.
struct Spined {
    report: SyncOutput,
    /// Whether the listing stopped at rest, its round left open.
    paused: bool,
    /// When paused, what the itemization of the spine that resumes it
    /// counts as owed: the bodies owed before this one, and the members it
    /// pulled, whose bodies may arrive in between.
    owed: HashSet<String>,
}

/// Reconciles one collection's spine, its bodies left to hydration.
///
/// Returns the report patches; a dry run stops after itemizing. The listing
/// tells `pager` each page it lands, which is how the bodies of a page
/// download while the next pages list.
///
/// A spine the pager pauses reports what its pages pulled and stops there,
/// its round open: the fetches, the staged work, the pushes and the
/// coverage wait for the spine that resumes it, which `owed` is handed
/// from the paused one.
///
/// Scans run in parallel, but `pushing` lets one collection of the source
/// push at a time: the two halves of a move derive from the store, and two
/// overlapping pushes would both read the create as pending, the target
/// uploading it while the source relocates it (pimdir SYNC §5).
#[allow(clippy::too_many_arguments)]
fn collection_spine(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    store_dir: &Path,
    pushing: &Mutex<()>,
    dry_run: bool,
    pager: &mut dyn Pager,
    owed: HashSet<String>,
) -> Result<Spined> {
    let mut report = SyncOutput::default();

    let loaded =
        load_side(store, collection).with_context(|| format!("Load {} {collection}", &ctx.name))?;
    let mut owed = owed;
    owed.extend(
        loaded
            .iter()
            .filter(|placement| {
                placement.status != PimdirStatus::Tombstone && placement.object.is_none()
            })
            .map(|placement| placement.handle.0.clone()),
    );
    let before: HashMap<String, PimdirFlags> = loaded
        .into_iter()
        .map(|placement| (placement.handle.0, placement.flags))
        .collect();

    let (pull, paused) = sync_side_rebuilding(collection, ctx, store, blobs, false, Some(pager))?;
    let display = display_name(&ctx.namespace, collection);
    // NOTE: before the report reads which conflicts survived, a divergence the
    // merge settles never having been a disagreement.
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
    if paused {
        owed.extend(pull.events.iter().filter_map(|event| match event {
            PimdirSyncEvent::Added(handle) | PimdirSyncEvent::ContentChanged(handle) => {
                Some(handle.0.clone())
            }
            _ => None,
        }));
        return Ok(Spined {
            report,
            paused,
            owed,
        });
    }
    itemize_fetches(
        collection,
        display,
        store,
        &ctx.name,
        &pull.events,
        &owed,
        &mut report,
    )?;
    itemize_single(collection, store, ctx, &mut report)?;
    if dry_run {
        return Ok(Spined {
            report,
            paused,
            owed: HashSet::new(),
        });
    }

    // NOTE: held across the passes, so a move's halves never push at once:
    // the second reads the first delivered, the target's create meeting the
    // relocated member as a probe, the source's remove no destination left.
    let push = pushing.lock().unwrap_or_else(PoisonError::into_inner);
    for _ in 0..=MAX_EXTRA_PASSES {
        let (pass, _) = sync_side_rebuilding(collection, ctx, store, blobs, ctx.writable(), None)?;
        if !moved(&pass) {
            break;
        }
    }
    drop(push);
    report_coverage(store, collection, ctx, &mut report);
    itemize_refused(&ctx.name, mem::take(&mut ctx.refused), &mut report);
    itemize_rejected(&ctx.name, mem::take(&mut ctx.rejected), &mut report);
    Ok(Spined {
        report,
        paused,
        owed: HashSet::new(),
    })
}

/// One hydrate batch: the collection it is read from, and the bodies
/// fetched together, each with the size the store records (0 when it
/// records none), which a backend reading by size plans its reads with.
#[derive(Debug, PartialEq)]
struct HydrateBatch {
    collection: String,
    bodies: Vec<(PimdirHandle, u64)>,
}

impl HydrateBatch {
    /// The batch of `targets`, read from `collection`.
    fn new<'t>(collection: &str, targets: impl IntoIterator<Item = &'t HydrateTarget>) -> Self {
        Self {
            collection: collection.to_string(),
            bodies: targets
                .into_iter()
                .map(|target| (target.handle.clone(), target.size))
                .collect(),
        }
    }
}

/// The bodies the servers answered gone this run, by collection.
///
/// They are owed nothing more: no later batch of the run asks for them
/// again, phase 2 included. The store keeps each member as it is until
/// the next listing of its collection takes it away (pimdir SYNC §5), so
/// a member gone is never fetched in a loop, and never dropped on the
/// word of a body read alone.
#[derive(Default)]
struct GoneBodies(Mutex<HashMap<String, HashSet<String>>>);

impl GoneBodies {
    fn record<'h>(&self, collection: &str, handles: impl IntoIterator<Item = &'h PimdirHandle>) {
        let mut gone = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let gone = gone.entry(collection.to_string()).or_default();
        gone.extend(handles.into_iter().map(|handle| handle.0.clone()));
    }

    fn holds(&self, collection: &str, handle: &PimdirHandle) -> bool {
        let gone = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        gone.get(collection)
            .is_some_and(|gone| gone.contains(&handle.0))
    }

    /// `targets` but the ones gone from `collection`.
    fn keep(&self, collection: &str, targets: Vec<HydrateTarget>) -> Vec<HydrateTarget> {
        let gone = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(gone) = gone.get(collection) else {
            return targets;
        };
        targets
            .into_iter()
            .filter(|target| !gone.contains(&target.handle.0))
            .collect()
    }
}

/// What the connections landing downloaded bodies share in a phase.
struct Landing<'a> {
    kind: Kind,
    blobs: &'a PimdirBlobs,
    /// Each collection's gate, which its spine holds but while it waits on
    /// the server (phase 1); none once every spine is over (phase 2).
    gates: Option<&'a HashMap<&'a str, Mutex<()>>>,
    /// Serialises the writes of the connections' store handles.
    applying: &'a Mutex<()>,
    /// The source's collections, when one handle names one item across
    /// them ([`Client::global_handles`]): Gmail's labels.
    shared: Option<&'a [String]>,
    gone: &'a GoneBodies,
    /// Called for each body of a batch once it is settled: downloaded,
    /// served from the store, gone, or owed nowhere any more.
    on_body: Option<&'a (dyn Fn() + Sync)>,
}

impl Landing<'_> {
    /// Downloads `batch` on `ctx`'s connection and raises each body to
    /// `Full` in every collection owing it, answering how many bodies came
    /// over the network.
    ///
    /// On a source whose handles are account-wide, every collection of
    /// the source is read for the batch's handles first: a handle one of
    /// them holds a body for is served from that body, the others are
    /// downloaded once, and each body lands in every collection holding
    /// its handle without one. A body the server answers gone is recorded
    /// for each collection owing it ([`GoneBodies`]) and asked for no more
    /// in the run. Only a failed download is an error; a write that fails
    /// is warned about, its bodies left for the next run.
    fn land(
        &self,
        ctx: &mut SourceCtx,
        store: &mut PimdirSourceStore,
        batch: &HydrateBatch,
    ) -> Result<usize> {
        let collection = batch.collection.as_str();
        let asked: Vec<PimdirHandle> = batch
            .bodies
            .iter()
            .map(|(handle, _)| handle)
            .filter(|handle| !self.gone.holds(collection, handle))
            .cloned()
            .collect();
        let placed = match self.shared {
            Some(collections) => Placed::read(store, collections, &asked).unwrap_or_else(|err| {
                warn!("cannot read where the bodies of {collection} are owed ({err:#}), landing them there alone");
                Placed::alone(collection, &asked)
            }),
            None => Placed::alone(collection, &asked),
        };

        let fetch = placed.to_fetch(&asked);
        let sizes: HashMap<&str, u64> = batch
            .bodies
            .iter()
            .filter(|(_, size)| *size > 0)
            .map(|(handle, size)| (handle.as_str(), *size))
            .collect();
        let sized = |id: &str| sizes.get(id).copied();
        let (mut items, gone) = match fetch.is_empty() {
            true => (Vec::new(), Vec::new()),
            false => {
                let hydrated = hydrate_batch(
                    self.kind,
                    ctx.pool.primary(),
                    wire_name(&ctx.namespace, collection),
                    &fetch,
                    &sized,
                    self.blobs,
                    self.on_body,
                )?;
                ctx.pool.downloaded(body_bytes(&hydrated.items));
                (hydrated.items, hydrated.gone)
            }
        };
        let fetched = items.len();

        for (handle, hash) in placed.stored {
            match stored_item(self.kind, self.blobs, handle.clone(), hash) {
                Ok(Some(item)) => items.push(item),
                // NOTE: left owed, for the next batch or run to download.
                Ok(None) => debug!("the stored body of {} is gone", handle.as_str()),
                Err(err) => warn!("cannot serve {} from the store: {err:#}", handle.as_str()),
            }
        }
        if let Some(on_body) = self.on_body {
            (fetched..batch.bodies.len()).for_each(|_| on_body());
        }

        if !gone.is_empty() {
            let gone: HashSet<&str> = gone.iter().map(PimdirHandle::as_str).collect();
            for (place, handles) in &placed.owed {
                self.gone.record(
                    place,
                    handles
                        .iter()
                        .filter(|handle| gone.contains(handle.as_str())),
                );
            }
            info!(
                "{} bodies of {} gone from the server since they were listed, left to the next listing",
                gone.len(),
                display_name(&ctx.namespace, collection),
            );
        }

        let items: HashMap<String, PimdirFetchedItem> = items
            .into_iter()
            .map(|item| (item.handle.0.clone(), item))
            .collect();
        for (place, handles) in placed.owed {
            let landed: Vec<PimdirFetchedItem> = handles
                .iter()
                .filter_map(|handle| items.get(&handle.0).cloned())
                .collect();
            if landed.is_empty() {
                continue;
            }
            // NOTE: the gate first, the order every applier takes them in:
            // the collection's spine may be mid-page. One collection at a
            // time, so no applier ever holds two gates.
            let _gate = self
                .gates
                .and_then(|gates| gates.get(place.as_str()))
                .map(|gate| gate.lock().unwrap_or_else(PoisonError::into_inner));
            let _applying = self.applying.lock().unwrap_or_else(PoisonError::into_inner);
            let fallback = PimRemote::new(&mut ctx.pool, self.blobs.clone(), ctx.namespace.clone());
            if let Err(err) = apply_batch(store, fallback, &place, landed) {
                warn!("{place} write error: {err:#}");
            }
        }

        Ok(fetched)
    }
}

/// Where the bodies of a batch are owed.
struct Placed {
    /// Each collection holding some of the handles without a body, with
    /// those handles, in the run's order.
    owed: Vec<(String, Vec<PimdirHandle>)>,
    /// The body a collection already holds for a handle another owes.
    stored: Vec<(PimdirHandle, PimdirHash)>,
}

impl Placed {
    /// The handles owed by their own collection alone.
    fn alone(collection: &str, handles: &[PimdirHandle]) -> Self {
        Self {
            owed: vec![(collection.to_string(), handles.to_vec())],
            stored: Vec::new(),
        }
    }

    /// Where `handles` are owed across `collections`, on a source whose
    /// handles are account-wide.
    fn read(
        store: &PimdirSourceStore,
        collections: &[String],
        handles: &[PimdirHandle],
    ) -> Result<Self> {
        let mut owed = Vec::new();
        let mut owing: HashSet<String> = HashSet::new();
        let mut held: HashMap<String, PimdirHash> = HashMap::new();
        if handles.is_empty() {
            return Ok(Self {
                owed,
                stored: Vec::new(),
            });
        }
        for collection in collections {
            let id = PimdirCollectionId(collection.clone());
            let loaded = store
                .load(&id, &PimdirLoadScope::Handles(handles.to_vec()))
                .with_context(|| format!("Load {collection}"))?;
            let mut its = Vec::new();
            for placement in loaded.placements {
                if placement.status == PimdirStatus::Tombstone {
                    continue;
                }
                match placement.object {
                    Some(hash) => {
                        held.entry(placement.handle.0).or_insert(hash);
                    }
                    None => {
                        owing.insert(placement.handle.0.clone());
                        its.push(placement.handle);
                    }
                }
            }
            if !its.is_empty() {
                owed.push((collection.clone(), its));
            }
        }
        let stored = handles
            .iter()
            .filter(|handle| owing.contains(&handle.0))
            .filter_map(|handle| Some((handle.clone(), held.remove(&handle.0)?)))
            .collect();
        Ok(Self { owed, stored })
    }

    /// The handles of `asked` to download: owed somewhere, and held in
    /// no collection already, in the order asked.
    fn to_fetch(&self, asked: &[PimdirHandle]) -> Vec<PimdirHandle> {
        let owed: HashSet<&str> = self
            .owed
            .iter()
            .flat_map(|(_, handles)| handles.iter().map(PimdirHandle::as_str))
            .collect();
        let stored: HashSet<&str> = self
            .stored
            .iter()
            .map(|(handle, _)| handle.as_str())
            .collect();
        asked
            .iter()
            .filter(|handle| owed.contains(handle.as_str()) && !stored.contains(handle.as_str()))
            .cloned()
            .collect()
    }
}

/// Phase 2, hydrating every body through one global work-stealing pool.
///
/// A batched fetch stays within one selected collection, so bodies are chunked
/// per collection and the batches queued in the chosen order across the
/// account. A worker steals the next, idling at no collection edge, each
/// on its own connection and store handle, the writes serialised.
/// Each batch is raised to `Full` as soon as it is fetched, so its bodies are
/// readable at once and an interrupted run keeps them. On a source whose
/// handles are account-wide (`shared`), a body is downloaded once for
/// every collection holding it ([`Landing::land`]).
#[allow(clippy::too_many_arguments)]
fn phase2_hydrate(
    source: &str,
    plans: Vec<CollectionPlan>,
    pacing: &Pacing,
    ctxs: &mut [SourceCtx],
    stores: &mut [PimdirSourceStore],
    blobs: &PimdirBlobs,
    shared: Option<&[String]>,
    gone: &GoneBodies,
) -> Result<()> {
    let batches = hydrate_batches(plans, pacing.order, pacing.prime, shared.is_some());
    let total_bodies: usize = batches.iter().map(|batch| batch.bodies.len()).sum();
    if total_bodies == 0 {
        return Ok(());
    }

    let kind = ctxs
        .first_mut()
        .map(|ctx| resolve_kind(&mut ctx.pool))
        .unwrap_or(Kind::Mail);

    let done = AtomicUsize::new(0);
    let fetched = AtomicUsize::new(0);
    let s = Spinner::start(format!("Downloading {source} 0% (0/{total_bodies})"));
    let on_body = || {
        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
        let percent = (n * 100).checked_div(total_bodies).unwrap_or(100);
        s.set_message(format!(
            "Downloading {source} {percent}% ({n}/{total_bodies})"
        ));
    };
    let applying = Mutex::new(());
    let landing = Landing {
        kind,
        blobs,
        gates: None,
        applying: &applying,
        shared,
        gone,
        on_body: Some(&on_body),
    };

    let mut workers: Vec<(&mut SourceCtx, &mut PimdirSourceStore)> =
        ctxs.iter_mut().zip(stores.iter_mut()).collect();
    hydrate_pool(&mut workers, batches, |(ctx, store), batch| {
        let n = landing.land(ctx, store, &batch)?;
        fetched.fetch_add(n, Ordering::Relaxed);
        Ok(())
    })?;

    s.success(format!(
        "Downloaded {} item(s) from {source}",
        fetched.load(Ordering::Relaxed)
    ));
    Ok(())
}

/// Chunks each collection's bodies into batches, queued in `order`.
///
/// The order holds within a collection and, by each batch's first body (its
/// largest or its newest), across the account, ties going to the
/// collection the run takes first (`plans` comes in the run's order).
/// Under a positive `prime`, each collection's `prime` newest bodies come
/// ahead of every other body, so what a first pass could not download is
/// downloaded first. Under `shared`, a handle names one item across the
/// collections (Gmail): it is planned once, under the first collection
/// listing it, primed when any of them primes it.
fn hydrate_batches(
    plans: Vec<CollectionPlan>,
    order: DownloadOrder,
    prime: usize,
    shared: bool,
) -> Vec<HydrateBatch> {
    let mut names = Vec::with_capacity(plans.len());
    // NOTE: each body's tier, 0 for a collection's primed ones, and its
    // collection's rank.
    let mut tiered: Vec<(u8, usize, HydrateTarget)> = Vec::new();
    let mut planned: HashMap<String, usize> = HashMap::new();
    for (rank, (collection, mut targets)) in plans.into_iter().enumerate() {
        names.push(collection);
        targets.sort_by(|a, b| DownloadOrder::Newest.compare(a, b));
        for (at, target) in targets.into_iter().enumerate() {
            let tier = u8::from(at >= prime);
            if shared {
                if let Some(&first) = planned.get(&target.handle.0) {
                    let held = &mut tiered[first].0;
                    *held = (*held).min(tier);
                    continue;
                }
                planned.insert(target.handle.0.clone(), tiered.len());
            }
            tiered.push((tier, rank, target));
        }
    }
    drop(planned);

    // NOTE: stable, so ties keep a collection's newest-first order.
    tiered.sort_by(|(tier_a, rank_a, a), (tier_b, rank_b, b)| {
        rank_a
            .cmp(rank_b)
            .then(tier_a.cmp(tier_b))
            .then(order.compare(a, b))
    });
    let mut batches: Vec<(u8, usize, HydrateTarget, HydrateBatch)> = Vec::new();
    for group in tiered.chunk_by(|a, b| (a.0, a.1) == (b.0, b.1)) {
        let (tier, rank) = (group[0].0, group[0].1);
        for chunk in group.chunks(BATCH_SIZE) {
            let batch = HydrateBatch::new(&names[rank], chunk.iter().map(|(_, _, t)| t));
            batches.push((tier, rank, chunk[0].2.clone(), batch));
        }
    }
    batches.sort_by(|(tier_a, rank_a, a, _), (tier_b, rank_b, b, _)| {
        tier_a
            .cmp(tier_b)
            .then(order.compare(a, b))
            .then(rank_a.cmp(rank_b))
    });
    batches.into_iter().map(|(_, _, _, batch)| batch).collect()
}

/// Runs `batches` over one worker per connection, each landing its batch
/// before taking the next.
///
/// Fetches overlap across the workers; `land` serialises its writes. A
/// landing error (its download failed) stops the pool and fails the run,
/// every batch landed before it staying in the store; a write that fails
/// is warned about by `land`, its batch left for the next run.
fn hydrate_pool<W: Send>(
    workers: &mut [W],
    batches: Vec<HydrateBatch>,
    land: impl Fn(&mut W, HydrateBatch) -> Result<()> + Sync,
) -> Result<()> {
    let queue: SegQueue<HydrateBatch> = SegQueue::new();
    for batch in batches {
        queue.push(batch);
    }

    let failure: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let stop = AtomicBool::new(false);

    let queue_ref = &queue;
    let failure_ref = &failure;
    let stop_ref = &stop;
    let land_ref = &land;

    thread::scope(|scope| {
        for worker in workers.iter_mut() {
            scope.spawn(move || {
                while !stop_ref.load(Ordering::Relaxed) {
                    let Some(batch) = queue_ref.pop() else {
                        break;
                    };
                    if let Err(err) = land_ref(worker, batch) {
                        *failure_ref.lock().unwrap() = Some(err);
                        stop_ref.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            });
        }
    });

    match failure.into_inner().unwrap() {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Raises one fetched batch to `Full`, serving its bodies from the batch.
///
/// The retained store then holds each body for the app to read offline. A
/// handle the batch lacks is fetched through `fallback`.
fn apply_batch<R: PimdirRemote<Error = anyhow::Error>>(
    store: &mut PimdirSourceStore,
    fallback: R,
    collection: &str,
    items: Vec<PimdirFetchedItem>,
) -> Result<()> {
    let handles: Vec<PimdirHandle> = items.iter().map(|item| item.handle.clone()).collect();
    if handles.is_empty() {
        return Ok(());
    }
    let mut remote = CachedFetchRemote::new(collection, items, fallback);
    run_verb(
        store,
        &mut remote,
        PimdirUpgrade::new(collection.to_string(), handles, PimdirTier::Full),
    )
    .with_context(|| format!("Apply bodies {collection}"))?;
    Ok(())
}

/// A `handle -> flags` snapshot of a side's items, taken before a pull.
///
/// Its remote flag changes are then diffed out of the sync's per-item events.
fn flag_snapshot(
    store: &PimdirSourceStore,
    collection: &str,
    source: &str,
) -> Result<HashMap<String, PimdirFlags>> {
    Ok(load_side(store, collection)
        .with_context(|| format!("Load {source} {collection}"))?
        .into_iter()
        .map(|placement| (placement.handle.0, placement.flags))
        .collect())
}

/// Itemizes the remote-originated flag changes and removals a pull applied.
///
/// The pull applies them silently, the item reading `Clean` afterwards, so they
/// are recovered from the sync's per-item events. A new remote item is an
/// `Added` event but the pull plan already reports it as a `Fetch`.
fn itemize_pulled(
    events: &[PimdirSyncEvent],
    before: &HashMap<String, PimdirFlags>,
    store: &PimdirSourceStore,
    collection: &str,
    display: &str,
    source: &str,
    report: &mut SyncOutput,
) -> Result<()> {
    let after = flag_snapshot(store, collection, source)?;
    itemize_conflicted(events, store, collection, display, source, report)?;

    for event in events {
        match event {
            PimdirSyncEvent::FlagsChanged(handle) => {
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
            PimdirSyncEvent::Vanished(handle) => {
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
            _ => {}
        }
    }
    Ok(())
}

/// Names the divergences a pass parked, once each.
///
/// A `Conflicted` event says a placement entered conflict, not that it is still
/// in one, [`resolve_conflicts`] having run since, so the store is asked which
/// survived. It notes rather than pushes: one divergence stays one line.
fn itemize_conflicted(
    events: &[PimdirSyncEvent],
    store: &PimdirSourceStore,
    collection: &str,
    display: &str,
    source: &str,
    report: &mut SyncOutput,
) -> Result<()> {
    if !events
        .iter()
        .any(|event| matches!(event, PimdirSyncEvent::Conflicted(_)))
    {
        return Ok(());
    }

    let parked: HashSet<String> = conflicted_placements(store, collection, source)?
        .into_iter()
        .map(|placement| placement.handle.0)
        .collect();

    for event in events {
        let PimdirSyncEvent::Conflicted(handle) = event else {
            continue;
        };

        if !parked.contains(&handle.0) {
            continue;
        }

        report.note_conflict(ItemConflict {
            side: source.to_string(),
            collection: display.to_string(),
            id: handle.0.clone(),
        });
    }

    Ok(())
}

/// Itemizes one side's projection: the work a flag, move or compose staged.
fn itemize_single(
    collection: &str,
    store: &PimdirStore,
    ctx: &SourceCtx,
    report: &mut SyncOutput,
) -> Result<()> {
    let display = display_name(&ctx.namespace, collection);
    let view = projection_view(store, collection, &ctx.name)
        .with_context(|| format!("Project {} {display}", ctx.name))?;
    for placement in view {
        for hunk in placement_hunks(&ctx.name, &ctx.name, &ctx.namespace, display, &placement) {
            report.item.patch.push(PatchEntry::new(hunk, None));
        }
    }
    Ok(())
}

/// Reports the pull plan: every not-yet-`Full`, non-tombstone item, and every
/// item the pull brought in with its body.
///
/// A kind named by its body (DAV, Google, Graph contacts and events) arrives
/// with it, its page fetching the bodies before it lands, so the plan read
/// off the store alone would be empty for exactly what the run pulled: those
/// are read off the pull's events.
///
/// `owed` names what was bodiless before the pull: a body a landed page's
/// hydration kept since is this run's fetch too, though the pull named no
/// change for it (a page an earlier run listed).
fn itemize_fetches(
    collection: &str,
    display: &str,
    store: &PimdirSourceStore,
    source: &str,
    events: &[PimdirSyncEvent],
    owed: &HashSet<String>,
    report: &mut SyncOutput,
) -> Result<()> {
    let pulled: HashSet<&str> = events
        .iter()
        .filter_map(|event| match event {
            PimdirSyncEvent::Added(handle) | PimdirSyncEvent::ContentChanged(handle) => {
                Some(handle.as_str())
            }
            _ => None,
        })
        .collect();
    let view =
        load_side(store, collection).with_context(|| format!("Load {source} {collection}"))?;
    for placement in view {
        let arrived = placement.object.is_some()
            && (pulled.contains(placement.handle.as_str())
                || owed.contains(placement.handle.as_str()));
        if placement.status == PimdirStatus::Tombstone || (placement.object.is_some() && !arrived) {
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

/// Reconciles one collection until quiescent.
///
/// The collection fills a report of its own, folded into the account's through
/// [`SyncOutput::absorb`], the same fold the one-source path makes at its
/// barrier. One fold keeps an arm from being dropped on just one path.
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
    account: &mut SyncOutput,
) -> Result<()> {
    left_store
        .ensure_collection(collection, media_type)
        .with_context(|| format!("Declare kind for {collection}"))?;

    let mut report = SyncOutput::default();
    let outcome = sync_collection_into(
        collection,
        left,
        right,
        left_store,
        right_store,
        blobs,
        store_dir,
        dry_run,
        relay,
        progress,
        &mut report,
    );

    report_coverage(left_store, collection, left, &mut report);
    report_coverage(right_store, collection, right, &mut report);

    // NOTE: folded whether the collection reconciled or failed: what it
    // reported before it stopped is still what happened.
    account.absorb(report);

    outcome
}

/// The body of [`sync_collection`], filling that collection's own report.
#[allow(clippy::too_many_arguments)]
fn sync_collection_into(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    store_dir: &Path,
    dry_run: bool,
    relay: bool,
    progress: &CollectionProgress,
    report: &mut SyncOutput,
) -> Result<()> {
    // NOTE: the opening round pulls and never pushes. Its events are then
    // the remotes' own doing rather than the echo of a write this run just
    // made, and no push clears a projection before `itemize` reads it.
    reconcile_pass(
        collection,
        left,
        right,
        left_store,
        right_store,
        blobs,
        store_dir,
        false,
        dry_run,
        report,
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

    for _ in 0..=MAX_EXTRA_PASSES {
        let progressed = reconcile_pass(
            collection,
            left,
            right,
            left_store,
            right_store,
            blobs,
            store_dir,
            true,
            dry_run,
            report,
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
    itemize_rejected(&left.name, mem::take(&mut left.rejected), report);
    itemize_rejected(&right.name, mem::take(&mut right.rejected), report);

    Ok(())
}

/// Propagates the collection's cross-side copies, returning how many moved.
///
/// Retaining hydrates the body into the store for the projection to push;
/// relaying streams it server-to-server and keeps only the spine.
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
    report: &mut SyncOutput,
) -> Result<usize> {
    if relay {
        relay_copies(
            collection,
            left,
            right,
            left_store,
            right_store,
            progress,
            report,
        )
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

/// One cross-copy body to relay.
///
/// `size` is the exact octet length from the item's summary, so the target
/// append is length-prefixed without buffering the body.
struct RelayTarget {
    /// The name of the source holding the body.
    holding: String,
    handle: PimdirHandle,
    /// The cross-side identity, so a relay is itemized under the hydrating id.
    link: String,
    /// The identity the target addresses the new item by.
    ///
    /// Owned, because the link id it points into is dropped with the hub.
    hint: Option<String>,
    /// The minted part of the key, naming a copy apart from its twin.
    mint: Option<String>,
    size: usize,
    /// The markers to append the copy with, as its backend spells them.
    flags: Vec<Flag>,
    /// The same markers as the hub holds them, for the placement the
    /// relay records: [`Flag`] equality collapses two wire spellings into
    /// one, so a round trip through it can drop a marker the hub kept.
    marks: PimdirFlags,
    /// The item's cached summary, which the copy shares: the same message
    /// on the other side, so the same summary.
    summary: Option<PimdirSummary>,
}

/// The relay targets: a one-sided, never-hydrated item its far side may create.
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
        let Some(size) = summary_size(&item.summary) else {
            warn!(
                "relay skips {} in {collection}: no size in its summary",
                link.0
            );
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
            marks: item.flags.clone(),
            summary: item.summary.clone(),
        });
    }
    Ok(out)
}

/// The octet length a mail summary carries, when present.
///
/// The one kind whose summary records a size (pimdir STORAGE Annex A.1), and
/// the one kind a body crosses by relay.
fn summary_size(summary: &Option<PimdirSummary>) -> Option<usize> {
    match summary.as_ref()? {
        PimdirSummary::Mail(mail) => mail.size.map(|size| size as usize),
        _ => None,
    }
}

/// The date a mail summary carries; other kinds carry none.
fn summary_date(summary: &Option<PimdirSummary>) -> Option<String> {
    match summary.as_ref()? {
        PimdirSummary::Mail(mail) => mail.date.clone(),
        _ => None,
    }
}

/// Streams each cross-copy body between the sides, keeping only the spine.
///
/// A relay never reaches the projection the hydrating path reports from, so
/// each write is itemized here instead, or a run that relayed would report
/// writing nothing. A copy is recorded against the side that now holds it.
fn relay_copies(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    progress: &CollectionProgress,
    report: &mut SyncOutput,
) -> Result<usize> {
    let targets = relay_targets(
        left_store,
        resolve_kind(&mut left.pool),
        collection,
        (&left.name, left.perms.item.create),
        (&right.name, right.perms.item.create),
    )?;
    // NOTE: the hub id keys the store and the wire name is what a server
    // answers to, and both sides sync one namespace, so the single
    // stripped name serves the fetch, the append and the report alike.
    let wire = wire_name(&left.namespace, collection).to_string();
    let total = targets.len();
    let mut count = 0;

    for target in targets {
        let holds_left = target.holding == left.name;
        let (holding_pool, target_pool) = if holds_left {
            (&mut left.pool, &mut right.pool)
        } else {
            (&mut right.pool, &mut left.pool)
        };
        let (target_name, target_store) = if holds_left {
            (right.name.clone(), &mut *right_store)
        } else {
            (left.name.clone(), &mut *left_store)
        };

        let written = relay_one(holding_pool, target_pool, &wire, &target)
            .with_context(|| format!("Relay {} in {wire}", target.handle.0))?;

        bind_relayed(target_store, collection, &target, written)
            .with_context(|| format!("Record the relay of {} in {wire}", target.handle.0))?;

        report.item.patch.push(PatchEntry::new(
            ItemHunk::Copy {
                source_side: target.holding.clone(),
                target_side: target_name,
                collection: wire.clone(),
                source_id: target.link.clone(),
                flags: target.flags.iter().cloned().collect(),
                origin: None,
                content_key: content_key(&target.link),
            },
            None,
        ));
        count += 1;
        progress.tick(count, total);
    }
    Ok(count)
}

/// Records the copy the target holds, bound to the item it was relayed from.
///
/// The relay is a side channel around the engine, so it owes the store the
/// assigned handle under the item's own link id, or the item stays one-sided
/// and every later pass relays it again. `collection` is the hub id.
fn bind_relayed(
    store: &mut PimdirSourceStore,
    collection: &str,
    target: &RelayTarget,
    written: WrittenItem,
) -> Result<()> {
    let placement = PimdirPlacement {
        collection: PimdirCollectionId(collection.to_string()),
        handle: PimdirHandle(written.id),
        link_id: Some(PimdirLinkId(target.link.clone())),
        // NOTE: relaying keeps the spine and nothing else, so the copy
        // holds no body: `Full` here would strand it, an upgrade skipping
        // whatever reads as hydrated.
        object: None,
        level: PimdirLevel::Meta,
        summary: target.summary.clone(),
        // NOTE: the relay derives no key, as a queued create does not,
        // and an unknown one leaves the hub's own in place.
        sort_key: PimdirSortKey::default(),
        flags: target.marks.clone(),
        status: PimdirStatus::Clean,
        conflict_revision: None,
        conflict_object: None,
        // NOTE: the base is the append the server has just accepted, so
        // the next sync of this side reconciles the copy as already in
        // sync. A base-less binding reads as a pending create instead,
        // which is the append again.
        base: Some(PimdirBase {
            flags: target.marks.clone(),
            revision: written.revision,
            object: None,
        }),
        origin: None,
    };

    store
        .write(vec![PimdirWriteOp::UpsertPlacement(placement)])
        .map_err(|err| anyhow!("Storage write error: {err}"))
}

/// Relays one message: a fetch thread into the pipe, this one into the append.
///
/// The body crosses without ever being held whole or stored. Both ends are
/// wire calls, so `collection` is the name the servers answer to; what the
/// target assigned is returned for the store to record.
fn relay_one(
    holding_pool: &mut Pool,
    target_pool: &mut Pool,
    collection: &str,
    target: &RelayTarget,
) -> Result<WrittenItem> {
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
    append.context("relay append")
}

/// One reconcile round over both sides, answering whether either moved.
///
/// A side's divergences are itemized right after its own merge, the engine's
/// events being this pass's alone. The endpoints are then reconciled with each
/// other, which no side's own merge sees: each agrees with its own server.
#[allow(clippy::too_many_arguments)]
fn reconcile_pass(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    store_dir: &Path,
    push: bool,
    dry_run: bool,
    report: &mut SyncOutput,
) -> Result<bool> {
    let reconciles = parks_divergences(left, right, dry_run);
    let ancestors = match reconciles {
        true => shared_bodies(left_store, collection)?,
        false => HashMap::new(),
    };
    let divergences = match reconciles {
        true => hub_divergences(left_store, collection)?,
        false => HashMap::new(),
    };

    let left_report = reconcile_side(
        collection, left, left_store, blobs, store_dir, push, dry_run, report,
    )?;
    let right_report = reconcile_side(
        collection,
        right,
        right_store,
        blobs,
        store_dir,
        push,
        dry_run,
        report,
    )?;

    let parked = park_divergences(
        collection,
        left,
        right,
        left_store,
        right_store,
        blobs,
        store_dir,
        &ancestors,
        dry_run,
        report,
    )? + park_hub_conflicts(
        collection,
        left,
        right,
        left_store,
        right_store,
        &divergences,
        dry_run,
        report,
    )?;

    Ok(moved(&left_report) || moved(&right_report) || parked > 0)
}

/// Whether a pass reconciles the two endpoints with each other.
///
/// Only where neither decides: under `one-way` the source is the truth and a
/// difference is not a divergence. A dry run stages nothing, the same bar
/// [`resolve_conflicts`] holds.
fn parks_divergences(left: &SourceCtx, right: &SourceCtx, dry_run: bool) -> bool {
    !dry_run && left.authority == Authority::Shared && right.authority == Authority::Shared
}

/// The body each shared item holds, which is what its two endpoints agreed on.
///
/// Read before a round pulls, because pulling loses it: a remote change drops
/// the stale body from the item and from that source's base, so once both have
/// pulled the store holds two new bodies and nothing they both came from.
fn shared_bodies(store: &PimdirStore, collection: &str) -> Result<HashMap<String, PimdirHash>> {
    let hub = store
        .load_hub(collection)
        .with_context(|| format!("Load the hub of {collection}"))?;

    Ok(hub
        .items
        .iter()
        .filter(|(_, item)| !item.deleted)
        .filter_map(|(link, item)| Some((link.0.clone(), item.object.clone()?)))
        .collect())
}

/// The diverging body each item already carries, before a round records one.
///
/// The hub keeps its cross-source flag once set, so the flag outlives the
/// decision that settled it. Read before the round, it tells what the round
/// newly recorded, which is the divergence a person has not seen yet.
fn hub_divergences(store: &PimdirStore, collection: &str) -> Result<HashMap<String, PimdirHash>> {
    let hub = store
        .load_hub(collection)
        .with_context(|| format!("Load the hub of {collection}"))?;

    Ok(hub
        .items
        .iter()
        .filter(|(_, item)| item.conflicted)
        .filter_map(|(link, item)| Some((link.0.clone(), item.conflict_object.clone()?)))
        .collect())
}

/// One item both endpoints rewrote since they last agreed on it.
struct Divergence {
    /// The cross-side identity, which names the item in a report.
    link: String,
    /// The body both endpoints came from, the merge's common ancestor.
    ancestor: PimdirHash,
    /// The handle the source holds it under, whose body becomes the shared one.
    source_handle: PimdirHandle,
    /// The handle the target holds it under, whose body is the divergence.
    target_handle: PimdirHandle,
    /// The revision the target's own pull observed, which a decision about the
    /// divergence is computed against and refused on once it moves.
    target_revision: Option<String>,
    /// The markers the target last agreed on, so parking moves the content axis
    /// and nothing else.
    target_flags: PimdirFlags,
}

/// The items both endpoints changed in one round, read from the store alone.
///
/// A pull drops the shared body and that source's base together, so an item
/// holding a base before the round and none after is one both rewrote. One
/// endpoint alone leaves the other's base in place, which tells them apart.
fn diverged_items(
    store: &PimdirStore,
    collection: &str,
    ancestors: &HashMap<String, PimdirHash>,
    source: &str,
    target: &str,
) -> Result<Vec<Divergence>> {
    let hub = store
        .load_hub(collection)
        .with_context(|| format!("Load the hub of {collection}"))?;
    let (source_id, target_id) = (source_id(source), source_id(target));
    let mut out = Vec::new();

    for (link, item) in &hub.items {
        if item.deleted || item.object.is_some() {
            continue;
        }
        let (Some(ancestor), Some(held), Some(diverging)) = (
            ancestors.get(&link.0),
            item.sources.get(&source_id),
            item.sources.get(&target_id),
        ) else {
            continue;
        };
        let pulled = |binding: &PimdirBinding| {
            binding
                .base
                .as_ref()
                .is_some_and(|base| base.object.is_none())
        };
        if !pulled(held) || !pulled(diverging) {
            continue;
        }

        out.push(Divergence {
            link: link.0.clone(),
            ancestor: ancestor.clone(),
            source_handle: held.handle.clone(),
            target_handle: diverging.handle.clone(),
            target_revision: diverging.base.as_ref().and_then(|b| b.revision.clone()),
            target_flags: diverging
                .base
                .as_ref()
                .map(|base| base.flags.clone())
                .unwrap_or_default(),
        });
    }

    Ok(out)
}

/// Reconciles what both endpoints rewrote, through the three-way merge.
///
/// Neither endpoint's own merge sees it: each agrees with its own server, so
/// the hub would project the last body it kept over the other. The pair gets
/// a one-endpoint divergence's shape instead; what no merge settles parks.
#[allow(clippy::too_many_arguments)]
fn park_divergences(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    store_dir: &Path,
    ancestors: &HashMap<String, PimdirHash>,
    dry_run: bool,
    report: &mut SyncOutput,
) -> Result<usize> {
    if !parks_divergences(left, right, dry_run) {
        return Ok(0);
    }

    let diverged = diverged_items(left_store, collection, ancestors, &left.name, &right.name)?;
    if diverged.is_empty() {
        return Ok(0);
    }

    debug!(
        "{} item(s) in {collection} changed on both endpoints",
        diverged.len()
    );

    // NOTE: the source's body becomes the shared one, which is what makes it
    // the merge's left side: ours, in the merge's own vocabulary.
    let mut remote = PimRemote::new(&mut left.pool, blobs.clone(), left.namespace.clone());
    let handles = diverged.iter().map(|d| d.source_handle.clone()).collect();
    run_verb(
        left_store,
        &mut remote,
        PimdirUpgrade::new(collection.to_string(), handles, PimdirTier::Full),
    )
    .with_context(|| format!("Fetch the diverging bodies of {} {collection}", &left.name))?;

    let hub = left_store
        .load_hub(collection)
        .with_context(|| format!("Load the hub of {collection}"))?;
    let mut writes = Vec::new();
    let mut marked = Vec::new();

    for divergence in diverged {
        let Some(item) = hub.items.get(&PimdirLinkId(divergence.link.clone())) else {
            continue;
        };
        let Some(object) = item.object.clone() else {
            debug!(
                "{} in {collection} has no source body to merge",
                divergence.link
            );
            continue;
        };

        // NOTE: the ancestor is unreferenced between the pull that dropped it
        // and the write below that pins it again, so a collector that ran in
        // between leaves a divergence with no base. It parks, unmerged, rather
        // than being merged against a body nobody wrote.
        let base = blobs
            .get(&divergence.ancestor)
            .with_context(|| {
                format!(
                    "Read the shared body of {} in {collection}",
                    divergence.link
                )
            })?
            .is_some()
            .then_some(divergence.ancestor);

        writes.push(PimdirWriteOp::UpsertPlacement(PimdirPlacement {
            collection: PimdirCollectionId(collection.to_string()),
            handle: divergence.target_handle.clone(),
            link_id: Some(PimdirLinkId(divergence.link)),
            object: Some(object),
            level: PimdirLevel::Full,
            summary: item.summary.clone(),
            sort_key: item.sort_key.clone(),
            flags: item.flags.clone(),
            status: PimdirStatus::Conflict,
            conflict_revision: divergence.target_revision.clone(),
            conflict_object: None,
            base: Some(PimdirBase {
                flags: divergence.target_flags,
                revision: divergence.target_revision,
                object: base,
            }),
            origin: None,
        }));
        marked.push(divergence.target_handle);
    }

    if writes.is_empty() {
        return Ok(0);
    }

    let staged = writes.len();
    right_store
        .write(writes)
        .with_context(|| format!("Park the divergences of {} {collection}", &right.name))?;

    resolve_conflicts(collection, right, right_store, blobs, store_dir, dry_run)?;

    let still: HashSet<String> = conflicted_placements(right_store, collection, &right.name)?
        .into_iter()
        .map(|placement| placement.handle.0)
        .collect();
    let display = display_name(&right.namespace, collection);
    for handle in marked {
        if still.contains(&handle.0) {
            report.note_conflict(ItemConflict {
                side: right.name.clone(),
                collection: display.to_string(),
                id: handle.0,
            });
        }
    }

    Ok(staged)
}

/// One item two endpoints disagree about with nothing behind them.
struct HubConflict {
    /// The cross-side identity, which names the item in a report.
    link: String,
    /// The body the hub kept as the shared one, which the source contributed.
    shared: PimdirHash,
    /// The body the hub recorded as diverging from it, the target's own.
    diverging: PimdirHash,
    /// The summary the shared body carries, so parking restates it rather than
    /// dropping it.
    summary: Option<PimdirSummary>,
    /// The key the shared body sorts by, restated for the same reason.
    sort_key: PimdirSortKey,
    /// The markers the item carries, restated for the same reason.
    flags: PimdirFlags,
    /// The handle the target holds the item under, whose placement parks.
    target_handle: PimdirHandle,
    /// The revision the target's own pull observed, which a decision about the
    /// divergence is computed against and refused on once it moves.
    target_revision: Option<String>,
    /// The markers the target last agreed on, so parking moves the content axis
    /// and nothing else.
    target_flags: PimdirFlags,
}

/// The divergences the round itself recorded between the two endpoints.
///
/// The hub's cross-source axis, kept on the shared item: both endpoints held
/// one identity before the store read either. Read against `before` so a
/// divergence parks once, the flag outliving the decision that settles it.
fn hub_conflicts(
    store: &PimdirStore,
    collection: &str,
    before: &HashMap<String, PimdirHash>,
    source: &str,
    target: &str,
) -> Result<Vec<HubConflict>> {
    let hub = store
        .load_hub(collection)
        .with_context(|| format!("Load the hub of {collection}"))?;
    let (source_id, target_id) = (source_id(source), source_id(target));
    let mut out = Vec::new();

    for (link, item) in &hub.items {
        if item.deleted || !item.conflicted {
            continue;
        }

        let (Some(shared), Some(diverging)) = (item.object.clone(), item.conflict_object.clone())
        else {
            continue;
        };
        if before.get(&link.0) == Some(&diverging) {
            continue;
        }
        let (Some(_), Some(binding)) = (item.sources.get(&source_id), item.sources.get(&target_id))
        else {
            continue;
        };
        if binding.conflicted {
            continue;
        }
        let base = binding.base.as_ref();

        out.push(HubConflict {
            link: link.0.clone(),
            shared,
            diverging,
            summary: item.summary.clone(),
            sort_key: item.sort_key.clone(),
            flags: item.flags.clone(),
            target_handle: binding.handle.clone(),
            target_revision: base.and_then(|base| base.revision.clone()),
            target_flags: base.map(|base| base.flags.clone()).unwrap_or_default(),
        });
    }

    Ok(out)
}

/// Parks what the two endpoints disagreed about before they agreed on anything.
///
/// Unlike [`park_divergences`] there is no common ancestor: two servers held
/// one identity under two bodies before the store read either, so a merge
/// could only park. Marking it also keeps the source's body off the target.
#[allow(clippy::too_many_arguments)]
fn park_hub_conflicts(
    collection: &str,
    left: &SourceCtx,
    right: &SourceCtx,
    left_store: &PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    before: &HashMap<String, PimdirHash>,
    dry_run: bool,
    report: &mut SyncOutput,
) -> Result<usize> {
    if !parks_divergences(left, right, dry_run) {
        return Ok(0);
    }

    let conflicts = hub_conflicts(left_store, collection, before, &left.name, &right.name)?;
    if conflicts.is_empty() {
        return Ok(0);
    }

    debug!(
        "{} item(s) in {collection} differ across the endpoints with no shared ancestor",
        conflicts.len()
    );

    let mut writes = Vec::with_capacity(conflicts.len());
    let mut marked = Vec::with_capacity(conflicts.len());

    for conflict in conflicts {
        marked.push(conflict.target_handle.0.clone());
        writes.push(PimdirWriteOp::UpsertPlacement(PimdirPlacement {
            collection: PimdirCollectionId(collection.to_string()),
            handle: conflict.target_handle,
            link_id: Some(PimdirLinkId(conflict.link)),
            object: Some(conflict.shared),
            level: PimdirLevel::Full,
            summary: conflict.summary,
            sort_key: conflict.sort_key,
            flags: conflict.flags,
            status: PimdirStatus::Conflict,
            conflict_revision: conflict.target_revision.clone(),
            conflict_object: Some(conflict.diverging),
            // NOTE: the base carries no body, the two endpoints never having
            // agreed on one. It is also what keeps the automatic merge out:
            // merging against the target's own body as the base would read the
            // target as having changed nothing and settle on the source's body,
            // which is the overwrite parking exists to refuse.
            base: Some(PimdirBase {
                flags: conflict.target_flags,
                revision: conflict.target_revision,
                object: None,
            }),
            origin: None,
        }));
    }

    let staged = writes.len();
    right_store.write(writes).with_context(|| {
        format!(
            "Park the endpoint divergences of {} {collection}",
            &right.name
        )
    })?;

    let display = display_name(&right.namespace, collection);
    for handle in marked {
        report.note_conflict(ItemConflict {
            side: right.name.clone(),
            collection: display.to_string(),
            id: handle,
        });
    }

    Ok(staged)
}

/// One side of a reconcile round: its merge, its probes and its report.
///
/// Only a pulling round's events name what the remote did. A pushing round's
/// `FlagsChanged` and `Vanished` also echo this run's own writes, so itemizing
/// them would report the run's work back as the remote's.
#[allow(clippy::too_many_arguments)]
fn reconcile_side(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    store_dir: &Path,
    push: bool,
    dry_run: bool,
    report: &mut SyncOutput,
) -> Result<PimdirSyncReport> {
    let push = push && !dry_run && ctx.writable();
    let before = if push {
        None
    } else {
        Some(flag_snapshot(store, collection, &ctx.name)?)
    };

    let (side, _) = sync_side_rebuilding(collection, ctx, store, blobs, push, None)?;
    resolve_conflicts(collection, ctx, store, blobs, store_dir, dry_run)?;

    let display = display_name(&ctx.namespace, collection);

    match &before {
        Some(before) => itemize_pulled(
            &side.events,
            before,
            store,
            collection,
            display,
            &ctx.name,
            report,
        )?,
        None => itemize_conflicted(&side.events, store, collection, display, &ctx.name, report)?,
    }

    Ok(side)
}

/// Whether a side's sync changed anything, used to detect convergence.
fn moved(report: &PimdirSyncReport) -> bool {
    report.pulled > 0 || report.pushed > 0
}

/// Runs one side's sync, then the handle-space rebuild guard.
///
/// The stored checkpoint's epoch is read before and after; a change means the
/// server renumbered every handle. The guard is pre/post within one run, so a
/// crash in between can miss one bump; link ids still converge on the next.
/// Also answers whether `pager` paused the sync, its round left open.
fn sync_side_rebuilding(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    push: bool,
    pager: Option<&mut dyn Pager>,
) -> Result<(PimdirSyncReport, bool)> {
    let pre = stored_epoch(ctx.pool.primary(), store, collection)?;

    let synced = sync_side(collection, ctx, store, blobs, push, pager)?;

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

    Ok(synced)
}

/// The handle-space epoch of `collection`'s stored checkpoint, if any.
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
            &PimdirCollectionId(collection.to_string()),
            &PimdirLoadScope::All,
        )
        .map_err(|err| anyhow!("Load {collection} checkpoint error: {err}"))?;
    Ok(loaded
        .checkpoint
        .as_ref()
        .and_then(|checkpoint| client.handle_space_epoch(&checkpoint.0)))
}

/// Drives the engine's rekey, returning its report and the new generation.
///
/// The rebuild batch drops every old handle as `Rekeyed`, which makes the
/// store bump the collection's generation in the same transaction (pimdir
/// SYNC §8), so "the ids you cached are void" commits with what voided them.
fn rebuild_collection(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
) -> Result<(PimdirRekeyReport, i64)> {
    let mut remote = PimRemote::new(&mut ctx.pool, blobs.clone(), ctx.namespace.clone());
    let report = run_verb(store, &mut remote, PimdirRekey::new(collection.to_string()))
        .with_context(|| format!("Rebuild {collection}"))?;
    let generation = store
        .generation(collection)
        .with_context(|| format!("Read the generation of {collection}"))?
        .context("Rekey completed without a write")?;

    Ok((report, generation))
}

/// The `sync` verb one side runs, under its options.
///
/// Being hub-bound fixes the delete disposition: a refused delete is held, not
/// reverted. Reverting says "this source still holds the member", and an add
/// beats a delete, so a side taking no deletes would resurrect on both sides.
fn sync_verb(
    collection: &str,
    push: bool,
    rights: PimdirPushRights,
    conflict: PimdirConflictPolicy,
    scope: PimdirScope,
    scope_bound: bool,
) -> PimdirSync {
    let opts = PimdirSyncOptions {
        push,
        rights,
        conflict,
        scope,
        ..Default::default()
    };

    PimdirSync::new(collection.to_string(), opts)
        .beside_other_sources(true)
        .scope_bound(scope_bound)
}

/// Runs one side's `sync` against its server and returns its report, and
/// whether `pager` paused it.
fn sync_side(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    push: bool,
    pager: Option<&mut dyn Pager>,
) -> Result<(PimdirSyncReport, bool)> {
    let bound = store
        .load(
            &PimdirCollectionId(collection.to_string()),
            &PimdirLoadScope::All,
        )
        .map(Bound::from_loaded)
        .map_err(|err| anyhow!("Load {collection} error: {err}"))?;
    let verb = sync_verb(
        collection,
        push,
        ctx.push_rights(),
        ctx.conflict_policy(),
        ctx.scope.clone(),
        ctx.pool.primary().scope_bound(),
    )
    .heads(ctx.pool.primary().heads());
    let mut remote = PimRemote::new(&mut ctx.pool, blobs.clone(), ctx.namespace.clone())
        .with_bound(collection, bound)
        .with_bodies(!ctx.dry_run);
    let report = run_verb_paged(store, &mut remote, verb, pager);
    // NOTE: kept whether the pass succeeded or not: a later failure does not
    // unlearn a refusal the run already saw.
    let refused = remote.take_refused();
    let rejected = remote.take_rejected();
    ctx.refused.extend(refused);
    ctx.rejected.extend(rejected);

    match report.with_context(|| format!("Sync {} {collection}", &ctx.name))? {
        Paged::Done(report) => Ok((report, false)),
        Paged::Paused(verb) => Ok((verb.report().clone(), true)),
    }
}

/// One side's placements the engine left marked conflicted in `collection`.
fn conflicted_placements(
    store: &PimdirSourceStore,
    collection: &str,
    source: &str,
) -> Result<Vec<PimdirPlacement>> {
    Ok(load_side(store, collection)
        .with_context(|| format!("Load {source} {collection}"))?
        .into_iter()
        .filter(|placement| placement.status == PimdirStatus::Conflict)
        .collect())
}

/// Merges what nobody disagreed about, leaving parked only what needs a person.
///
/// Most divergence is not disagreement: one side changed a phone number and the
/// other a note, and the base proves it. The remote body is only wanted, not
/// held, so it is fetched first and never merged against nothing.
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

    // NOTE: the engine records a store-wins divergence as a conflict on a pass
    // that cannot push, and the pushing pass never revisits it, so the store's
    // body is restated here for that pass to push over the endpoint.
    if ctx.authority == Authority::Store {
        return keep_store_bodies(collection, &ctx.name, store, blobs);
    }

    debug!("merge {} conflicted item(s) in {collection}", parked.len());

    let wanted: Vec<PimdirHandle> = parked
        .iter()
        .filter(|placement| placement.conflict_object.is_none())
        .map(|placement| placement.handle.clone())
        .collect();

    if !wanted.is_empty() {
        let mut remote = PimRemote::new(&mut ctx.pool, blobs.clone(), ctx.namespace.clone());
        run_verb(
            store,
            &mut remote,
            PimdirUpgrade::new(collection.to_string(), wanted, PimdirTier::Full),
        )
        .with_context(|| format!("Fetch the diverging bodies of {} {collection}", &ctx.name))?;
    }

    let kind = resolve_kind(&mut ctx.pool);

    merge_conflicts(collection, kind, &ctx.name, store, blobs, store_dir)
}

/// Settles every divergence of a side the store decides on the store's body.
///
/// Restating the held body is the decision `conflict resolve --prefer-local`
/// takes: the conflict clears against the revision the endpoint holds, so the
/// next pushing pass overwrites it. Nothing is merged, the endpoint's own edit
/// being what `one-way` discards. The edit is the engine's own rather than an
/// owner's through the queue, which a one-way account refuses.
fn keep_store_bodies(
    collection: &str,
    source: &str,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
) -> Result<usize> {
    let mut kept = 0usize;

    for placement in conflicted_placements(store, collection, source)? {
        let handle = placement.handle;
        let name = handle.0.clone();

        let Some(hash) = placement.object else {
            debug!("conflicted item {name} in {collection} holds no body to keep");
            continue;
        };
        let Some(body) = blobs
            .get(&hash)
            .with_context(|| format!("Read the body of {name} in {collection}"))?
        else {
            debug!("the body of the conflicted item {name} in {collection} is not in the store");
            continue;
        };

        let object = PimdirObject {
            hash,
            size: body.len(),
        };
        let edit = PimdirMutation::Edit {
            handle,
            object,
            body,
            summary: None,
            sort_key: None,
        };
        store
            .mutate(collection, edit)
            .map_err(|err| anyhow!("Keep the store's body of {name} in {collection}: {err}"))?;

        kept += 1;
    }

    if kept > 0 {
        debug!("kept the store's body over {kept} divergence(s) in {collection}");
    }

    Ok(kept)
}

/// The half of [`resolve_conflicts`] that reaches no server.
///
/// Merges the placements whose three bodies the store holds and stages each
/// result through the queue, the same path an owner's own edit takes, so a
/// merged body is written exactly one way. What it cannot settle stays parked.
fn merge_conflicts(
    collection: &str,
    kind: Kind,
    source: &str,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    store_dir: &Path,
) -> Result<usize> {
    // NOTE: opened before the first blob write, its staging lock being what
    // keeps a collector out of the window between a body reaching the blob
    // tree and the queue row pinning it.
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

        let read = |hash: &PimdirHash| -> Result<Option<Vec<u8>>> {
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
        let object = PimdirObject {
            hash,
            size: size as usize,
        };

        producer
            .enqueue(
                collection,
                &PimdirAction::Update {
                    seq,
                    object: object.hash.clone(),
                },
                Some(&object),
            )
            .with_context(|| format!("Stage the merged body of {handle} in {collection}"))?;

        staged += 1;
    }

    drop(producer);

    if staged == 0 {
        return Ok(0);
    }

    let drained = store
        .drain()
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

/// Hydrates the bodies of one-sided items the other side may receive.
///
/// The hub then holds the body and its next projection stages the copy.
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
        HydrationSide {
            name: &left.name,
            creates: left.perms.item.create,
            updates: left.perms.item.update,
            decides: left.authority == Authority::Endpoint,
        },
        HydrationSide {
            name: &right.name,
            creates: right.perms.item.create,
            updates: right.perms.item.update,
            decides: right.authority == Authority::Endpoint,
        },
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
        run_verb(
            left_store,
            &mut remote,
            PimdirUpgrade::new(collection.to_string(), left_handles, PimdirTier::Full),
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
        run_verb(
            right_store,
            &mut remote,
            PimdirUpgrade::new(collection.to_string(), right_handles, PimdirTier::Full),
        )
        .with_context(|| format!("Hydrate bodies for {} {collection}", right.name))?;
    }
    Ok(total)
}

/// Itemizes the pending cross-side work from each side's hub projection.
///
/// A `Created` placement is a copy in, a `Dirty` one a flag change or an
/// update, a `Tombstone` a delete.
fn itemize(
    collection: &str,
    store: &PimdirStore,
    left: &SourceCtx,
    right: &SourceCtx,
    report: &mut SyncOutput,
) -> Result<()> {
    for (ctx, other) in [(left, right), (right, left)] {
        let display = display_name(&ctx.namespace, collection);
        let view = projection_view(store, collection, &ctx.name)
            .with_context(|| format!("Project {} {display}", ctx.name))?;
        for placement in view {
            for hunk in placement_hunks(&ctx.name, &other.name, &ctx.namespace, display, &placement)
            {
                report.item.patch.push(PatchEntry::new(hunk, None));
            }
        }
    }
    Ok(())
}

/// Maps a projected placement to its report hunks.
///
/// A flag change can surface as both an add and a remove.
fn placement_hunks(
    source: &str,
    other: &str,
    namespace: &str,
    collection: &str,
    placement: &PimdirPlacement,
) -> Vec<ItemHunk> {
    match placement.status {
        PimdirStatus::Created => {
            let link = placement
                .link_id
                .as_ref()
                .map(|l| l.0.clone())
                .unwrap_or_default();
            let flags = to_email_flag_set(&placement.flags);
            let content_key = content_key(&link);
            // NOTE: what the create delivers by says what it is (SYNC §5):
            // a server-side copy from its origin, an upload of a body the
            // other side holds, or, with no other side, an append a
            // frontend authored.
            let hunk = match &placement.origin {
                Some(origin) => ItemHunk::Copy {
                    source_side: source.to_string(),
                    target_side: source.to_string(),
                    collection: collection.to_string(),
                    source_id: link,
                    flags,
                    origin: Some(wire_name(namespace, &origin.collection.0).to_string()),
                    content_key,
                },
                None if source == other => ItemHunk::Add {
                    side: source.to_string(),
                    collection: collection.to_string(),
                    id: link,
                    flags,
                    content_key,
                },
                None => ItemHunk::Copy {
                    source_side: other.to_string(),
                    target_side: source.to_string(),
                    collection: collection.to_string(),
                    source_id: link,
                    flags,
                    origin: None,
                    content_key,
                },
            };
            vec![hunk]
        }
        PimdirStatus::Tombstone => vec![ItemHunk::Delete {
            side: source.to_string(),
            collection: collection.to_string(),
            id: placement.handle.0.clone(),
            content_key: 0,
        }],
        PimdirStatus::Dirty => {
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
        PimdirStatus::Clean | PimdirStatus::Conflict => Vec::new(),
    }
}

/// Names the creates a side refused because it already holds the identity.
///
/// A collection reconciles until quiescent, so a create the other side will not
/// take is re-refused every pass. Two copies of one identity are still two
/// refusals, which is what the handle tells apart from a repeated one.
fn itemize_refused(side: &str, refused: Vec<RefusedCreate>, report: &mut SyncOutput) {
    let mut seen: HashSet<(String, String, String)> = HashSet::new();

    for refused in refused {
        let key = (
            refused.collection.clone(),
            refused.uid.clone(),
            refused.handle,
        );

        if !seen.insert(key) {
            continue;
        }

        report.refused.push(RefusedDuplicate {
            side: side.to_string(),
            collection: refused.collection,
            uid: refused.uid,
        });
    }
}

/// Names the writes a side would not take, un-counting their hunks.
///
/// The item patch is a plan, itemized before anything is pushed, so a refused
/// write left in it counts as one the run made and comes back as a phantom on
/// every later run, as a rejected `PUT` did against a real CardDAV server.
fn itemize_rejected(side: &str, rejected: Vec<RejectedPush>, report: &mut SyncOutput) {
    let mut seen: HashSet<(String, String, &'static str)> = HashSet::new();

    for rejected in rejected {
        // NOTE: the hub stages an append under a handle of its own making,
        // the item's link id with a marker appended, the item having no
        // handle on the side it is being created on. That marker is what a
        // person would read, and the copy hunk names the item by the link id
        // alone, so both the report and the retraction take the id back.
        let id = match rejected.handle.split_once('\u{1}') {
            Some((link, _)) => link.to_string(),
            None => rejected.handle.clone(),
        };
        let key = (rejected.collection.clone(), id.clone(), rejected.action);

        if !seen.insert(key) {
            continue;
        }

        report
            .item
            .patch
            .retain(|entry| !names(&entry.hunk, side, &rejected.collection, &id));
        report.rejected.push(RejectedWrite {
            side: side.to_string(),
            collection: rejected.collection,
            id,
            action: rejected.action.to_string(),
            reason: rejected.reason,
        });
    }
}

/// Whether `hunk` is the plan for `handle` in `collection` on `side`.
///
/// A copy carries no handle on the side it is being created on, so it is named
/// by the item's link id, which is what an append the hub staged is keyed by
/// too. A fetch reaches no server and is never rejected.
fn names(hunk: &ItemHunk, side: &str, collection: &str, handle: &str) -> bool {
    match hunk {
        ItemHunk::AddFlags {
            side: s,
            collection: c,
            id,
            ..
        }
        | ItemHunk::RemoveFlags {
            side: s,
            collection: c,
            id,
            ..
        }
        | ItemHunk::Delete {
            side: s,
            collection: c,
            id,
            ..
        }
        | ItemHunk::Update {
            side: s,
            collection: c,
            id,
            ..
        } => s == side && c == collection && id == handle,
        ItemHunk::Add {
            side: s,
            collection: c,
            id,
            ..
        } => s == side && c == collection && id == handle,
        ItemHunk::Copy {
            target_side,
            collection: c,
            source_id,
            ..
        } => target_side == side && c == collection && source_id == handle,
        ItemHunk::Fetch { .. } => false,
    }
}

/// The context of the named source in a pair.
fn source_ctx<'a>(
    name: &str,
    left: &'a mut SourceCtx,
    right: &'a mut SourceCtx,
) -> &'a mut SourceCtx {
    if left.name == name { left } else { right }
}

/// A source's collections: the id addressing one, against the name it goes by.
///
/// The two coincide on IMAP and Graph and part ways on DAV, where a segment is
/// routinely a UUID and the name is a `DAV:displayname`. Every hub id and wire
/// call is built from the key; nothing reads the value back.
fn list_collections(client: &mut Client) -> Result<BTreeMap<String, String>> {
    Ok(client
        .list_collections(false)
        .context("List collections error")?
        .into_iter()
        .map(|collection| (collection.id, collection.name))
        .collect())
}

/// Records what a collection is called, the id staying what addresses it.
///
/// Sources are read in declared order, the first with something to say
/// winning; one reporting no name leaves the bare id. Nothing keys on the
/// column, so a failure costs a label and never a sync, and is logged.
fn declare_name(
    store: &PimdirSourceStore,
    collection: &str,
    id: &str,
    listed: &[&BTreeMap<String, String>],
) {
    let name = listed
        .iter()
        .find_map(|names| {
            names
                .get(id)
                .map(String::as_str)
                .filter(|name| !name.trim().is_empty())
        })
        .unwrap_or(id);

    if let Err(err) = store.set_collection_name(collection, name) {
        warn!("cannot name {collection} `{name}`: {err}");
    }
}

/// The role each collection of `listed` states, by id: what the listing
/// carries, plus what [`Client::lookup_roles`] answers for a backend whose
/// listing does not, asked again only when the source's collection set
/// moved since it was last asked (kept in the sidecar). `None` when a
/// lookup failed: the roles recorded stay as they are rather than cleared.
fn stated_roles(
    lookup: impl FnOnce() -> Result<Option<BTreeMap<String, String>>>,
    source: &str,
    listed: &[Collection],
    state: &mut StoreState,
) -> Option<BTreeMap<String, String>> {
    let mut roles: BTreeMap<String, String> = listed
        .iter()
        .filter_map(|c| Some((c.id.clone(), c.role.clone()?)))
        .collect();

    let mut listing: Vec<String> = listed.iter().map(|c| c.id.clone()).collect();
    listing.sort();

    let looked_up = match state.looked_up_roles.get(source) {
        Some(cached) if cached.listing == listing => cached.roles.clone(),
        _ => match lookup() {
            Ok(None) => BTreeMap::new(),
            Ok(Some(found)) => {
                state.looked_up_roles.insert(
                    source.to_owned(),
                    LookedUpRoles {
                        listing,
                        roles: found.clone(),
                    },
                );
                found
            }
            Err(err) => {
                warn!("cannot look up the roles {source} states: {err:#}");
                return None;
            }
        },
    };

    roles.extend(looked_up);
    Some(roles)
}

/// Records the role a source states for a collection, or clears one it no
/// longer states. Only from the source, never from a name; nothing keys on
/// it, so a failure costs a role and never a sync, and is logged.
fn declare_role(store: &PimdirSourceStore, collection: &str, role: Option<&str>) {
    if let Err(err) = store.set_collection_role(collection, role) {
        warn!("cannot record the role of {collection} as {role:?}: {err}");
    }
}

/// The ids the filter keeps, matched on the id: a DAV collection is filtered
/// by its path segment, which is what a configuration names.
fn filter_collections(
    collections: &BTreeMap<String, String>,
    filter: &CollectionFilter,
) -> BTreeSet<String> {
    let matches = |id: &str, list: &[String]| list.iter().any(|f| f.eq_ignore_ascii_case(id));
    collections
        .keys()
        .filter(|id| match filter {
            CollectionFilter::All => true,
            CollectionFilter::Include(list) => matches(id, list),
            CollectionFilter::Exclude(list) => !matches(id, list),
        })
        .cloned()
        .collect()
}

/// One endpoint as the collection diff reads it: its name and what it lets
/// a run do to its collection set.
type DiffSide<'a> = (&'a str, &'a CollectionPermissions);

/// Three-way collection diff, gated by each side's permissions.
///
/// A collection one side lists and the other does not is new on the first,
/// and created on the other, unless the store knows both held it
/// (`held_by_both`, the base): then the other deleted it, and the delete
/// crosses instead. A delete the remaining side forbids leaves it there,
/// never created again where it was deleted.
fn diff_collections(
    left: &BTreeSet<String>,
    right: &BTreeSet<String>,
    held_by_both: &BTreeSet<String>,
    (left_name, left_perms): DiffSide<'_>,
    (right_name, right_perms): DiffSide<'_>,
) -> Vec<CollectionHunk> {
    let mut hunks = Vec::new();
    let sides = [
        (
            left.difference(right),
            (left_name, left_perms),
            right_name,
            right_perms,
        ),
        (
            right.difference(left),
            (right_name, right_perms),
            left_name,
            left_perms,
        ),
    ];
    for (only, (here, here_perms), there, there_perms) in sides {
        for collection in only {
            let hunk = match held_by_both.contains(collection) {
                true if here_perms.delete => CollectionHunk::Delete {
                    side: here.to_owned(),
                    collection: collection.clone(),
                },
                true => {
                    info!("{collection} was deleted on {there}, which {here} does not allow");
                    continue;
                }
                false if there_perms.create => CollectionHunk::Create {
                    side: there.to_owned(),
                    collection: collection.clone(),
                },
                false => continue,
            };
            hunks.push(hunk);
        }
    }
    hunks
}

/// The listed collections, among `names`, that the store knows both
/// endpoints held: the base of the three-way collection diff, read from the
/// sources holding a binding or a checkpoint in each (pimdir SYNC §5).
fn held_by_both<'a>(
    store: &PimdirSourceStore,
    namespace: &str,
    names: impl IntoIterator<Item = &'a String>,
    left: &str,
    right: &str,
) -> Result<BTreeSet<String>> {
    let mut held = BTreeSet::new();
    for name in names {
        let collection = hub_id(namespace, name);
        let sources = store
            .collection_sources(&collection)
            .with_context(|| format!("List the sources of {collection}"))?;
        if sources.iter().any(|source| source == left) && sources.iter().any(|s| s == right) {
            held.insert(name.clone());
        }
    }
    Ok(held)
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

fn to_email_flag_set(flags: &PimdirFlags) -> BTreeSet<Flag> {
    let Some(flags) = flags.known() else {
        return BTreeSet::new();
    };

    flags.iter().map(|s| Flag::from_raw(s.clone())).collect()
}

/// What `new` gained over `old`, and what it lost.
///
/// An unknown set holds no markers to compare with, so it reads as empty:
/// nothing is reported added or removed against a side nobody read.
fn flag_diff(old: &PimdirFlags, new: &PimdirFlags) -> (BTreeSet<Flag>, BTreeSet<Flag>) {
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

/// A u64 content key for the report DTO, never displayed, so only consistent.
fn content_key(link: &str) -> u64 {
    let mut acc: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in link.bytes() {
        acc ^= byte as u64;
        acc = acc.wrapping_mul(0x0000_0100_0000_01b3);
    }
    acc
}

/// Drains the pending frontend actions, the whole store's in append order.
///
/// The store applies each action as the source syncing its collection, not as
/// the handle draining (STORAGE §15.2), so whichever side runs first answers
/// for both. The report names the collections whose rows landed.
pub(crate) fn drain_queues(store: &mut PimdirSourceStore, report: &mut SyncOutput) {
    let before = match store.list_pending_actions() {
        Ok(rows) => rows,
        Err(err) => {
            warn!("cannot list the queued actions: {err}");
            return;
        }
    };
    if before.is_empty() {
        return;
    }

    let drained = match store.drain() {
        Ok(drained) => drained,
        Err(err) => {
            warn!("drain failed, actions stay queued: {err}");
            return;
        }
    };
    if drained.applied > 0 || drained.parked > 0 || drained.skipped > 0 {
        info!(
            "drained {} queued action(s) ({} parked, {} skipped)",
            drained.applied, drained.parked, drained.skipped
        );
    }
    if drained.applied == 0 {
        return;
    }

    let parked: BTreeSet<i64> = store
        .parked_actions()
        .map(|rows| rows.into_iter().map(|row| row.id).collect())
        .unwrap_or_default();
    let pending: BTreeSet<i64> = store
        .list_pending_actions()
        .map(|rows| rows.into_iter().map(|row| row.id).collect())
        .unwrap_or_default();
    let mut applied: BTreeMap<String, usize> = BTreeMap::new();
    for row in before {
        if !pending.contains(&row.id) && !parked.contains(&row.id) {
            *applied.entry(row.collection).or_default() += 1;
        }
    }
    report.drained.extend(
        applied
            .into_iter()
            .map(|(collection, applied)| DrainedQueue {
                collection,
                applied,
            }),
    );
}

/// Reports what one endpoint holds of a collection it just synced (pimdir
/// SYNC §5): the last closed round's scope and when, and a round left open.
fn report_coverage(
    store: &PimdirStore,
    collection: &str,
    ctx: &SourceCtx,
    report: &mut SyncOutput,
) {
    let rows = match store.list_coverage(collection) {
        Ok(rows) => rows,
        Err(err) => {
            warn!("cannot read the coverage of {collection}: {err}");
            return;
        }
    };

    for row in rows.into_iter().filter(|row| row.source == ctx.name) {
        let (since, until, at) = match row.coverage {
            Some(coverage) => (
                coverage.scope.since,
                coverage.scope.until,
                Some(coverage.at),
            ),
            None => (None, None, None),
        };
        report.coverage.push(CollectionCoverage {
            source: row.source,
            collection: display_name(&ctx.namespace, collection).to_owned(),
            since,
            until,
            at,
            round: row.round.map(|round| OpenRound {
                since: round.scope.since,
                until: round.scope.until,
                started_at: round.started_at,
            }),
        });
    }
}

/// Surfaces the store's parked queue actions, once for the run.
///
/// A parked row belongs to the store, not to a source, so reading it where the
/// drain runs reports it once per source: an account syncing mail, contacts and
/// calendar showed the same row three times. Every run re-reports them.
fn report_parked(store: &PimdirStore, report: &mut SyncOutput) {
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

/// Counts the decisions the store is holding, once for the run.
///
/// Read from the store rather than from the run's own tally: the engine says
/// nothing about a placement it already parked, so a run itemizes only what it
/// newly marked, which is why that tally cannot count what is waiting.
fn count_conflicts(store: &PimdirStore, account: &str, report: &mut SyncOutput) {
    match store.list_conflicts(Some(account)) {
        Ok(conflicts) => report.outstanding_conflicts = conflicts.len(),
        Err(err) => warn!("cannot count the conflicts waiting for a decision: {err}"),
    }
}

/// Warns about the conflicts this run marked, one line per item.
///
/// Only what this run marked, which is the whole of the once-only rule: a
/// five-minute schedule over one unresolved card warns once, not three hundred
/// times a day. A `--json` caller notifies off `conflicts`, keeping no state.
fn warn_conflicts(report: &SyncOutput) -> usize {
    for conflict in &report.conflicts {
        warn!("{conflict}");
    }

    report.conflicts.len()
}

/// One endpoint a run may send through: its name, and its open session when
/// the run opened one.
///
/// A run performing intents alone opens an endpoint only for an intent that
/// needs its session, so an SMTP channel sends with its source unopened.
struct SendSide<'a> {
    name: &'a str,
    #[cfg_attr(not(feature = "msgraph"), allow(dead_code))]
    client: Option<&'a mut Client>,
}

impl<'a> SendSide<'a> {
    /// A side whose session the run opened.
    fn open(ctx: &'a mut SourceCtx) -> Self {
        Self {
            name: &ctx.name,
            client: Some(ctx.pool.primary()),
        }
    }

    /// A side the run did not open.
    fn unopened(name: &'a str) -> Self {
        Self { name, client: None }
    }
}

/// Performs the queue's `submit` intents, which the store's drain leaves alone.
///
/// A sent intent is acknowledged with a receipt, releasing its body's pin, so a
/// producer reads it applied rather than withdrawn; a permanent failure parks
/// the row with its error, an uncertain one parks it under
/// [`submit::UNCERTAIN`], never to be sent again, and a transient one leaves
/// it pending. With no channel they stay pending, never parked, another build
/// being able to send.
///
/// Each attempt is counted, and the intent marked in flight
/// ([`submit::InFlight`]), before its first byte leaves: a producer reading
/// the row mid-send sees the attempt, and a run killed mid-send leaves the
/// mark, so the next one parks that intent as uncertain instead of sending
/// it again. The mark is cleared once the outcome is in the store.
fn drain_submits(
    account_config: &AccountConfig,
    account: &Account,
    sides: &mut [SendSide<'_>],
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobs,
    dir: &Path,
    report: &mut SyncOutput,
) {
    let intents = match submit::pending(store) {
        Ok(intents) => intents,
        Err(err) => {
            warn!("cannot read the queued submit intents: {err:#}");
            return;
        }
    };
    let in_flight = submit::InFlight::new(dir);
    let Some(intents) = park_interrupted(store, &in_flight, intents, report) else {
        return;
    };
    // NOTE: an intent naming another source is that source's to send, on its
    // own run (pimdir STORAGE §15.6); one naming none predates the field.
    let sender = sending_source(account_config, sides);
    let intents: Vec<_> = intents
        .into_iter()
        .filter(|intent| {
            intent
                .source()
                .is_none_or(|source| Some(&source) == sender.as_ref())
        })
        .collect();
    if intents.is_empty() {
        return;
    }
    info!("performing {} queued submit intent(s)", intents.len());
    // NOTE: a provider filing what it sends (Gmail, Graph) answers a copy
    // by itself; any other gets it as an `add`, queued once sent (pimdir
    // STORAGE Annex B.2).
    let files_copy = sender.as_ref().is_some_and(|sender| {
        account_config
            .sources()
            .ok()
            .and_then(|sources| sources.get(sender).map(capability::files_sent_copy))
            .unwrap_or(false)
    });

    #[cfg(any(feature = "smtp", feature = "msgraph"))]
    {
        let entry =
            |intent: &submit::SubmitIntent, error: Option<String>, parked, uncertain| SubmitEntry {
                id: intent.id,
                collection: intent.collection.clone(),
                subject: intent.subject(),
                error,
                parked,
                uncertain,
            };

        let mut channel = match open_send_channel(account_config, account, sides, intents.len()) {
            Ok(channel) => channel,
            Err(Unopened::Pending) => return,
            Err(Unopened::Unreached(unreached)) => {
                report.unreached.push(unreached);
                return;
            }
            Err(Unopened::Refused(error)) => {
                // NOTE: nothing was attempted, so no attempt is counted:
                // the producer reads the rows as never tried.
                for intent in &intents {
                    report
                        .submitted
                        .push(entry(intent, Some(error.clone()), false, false));
                }
                return;
            }
        };

        let mut sent = 0;
        let mut copies = 0;
        // NOTE: why the run stops attempting, once it must: an outcome it
        // could not record keeps its mark, which the next intent's would
        // overwrite.
        let mut halted: Option<String> = None;
        for intent in &intents {
            if let Some(why) = halted.as_deref().or(channel.lost()) {
                let error = format!("Not attempted, {why}");
                report
                    .submitted
                    .push(entry(intent, Some(error), false, false));
                continue;
            }

            let outgoing = match submit::prepare(blobs, intent) {
                Ok(outgoing) => outgoing,
                Err(failure) => {
                    let reason = failure.reason();
                    warn!("submit intent #{} failed: {reason}", intent.id);
                    let parked = failure.parks();
                    if let Err(err) = store.fail_action(intent.id, parked.then_some(&*reason)) {
                        warn!("cannot record submit intent #{} failure: {err}", intent.id);
                    }
                    report
                        .submitted
                        .push(entry(intent, Some(reason), parked, false));
                    continue;
                }
            };

            // NOTE: counted and marked before the first byte leaves; a row
            // whose attempt cannot be recorded is not sent.
            let marked = store
                .fail_action(intent.id, None)
                .map_err(|err| format!("cannot record the attempt: {err}"))
                .and_then(|()| {
                    in_flight
                        .begin(intent.id)
                        .map_err(|err| format!("cannot mark it in flight: {err}"))
                });
            if let Err(why) = marked {
                warn!("submit intent #{} not sent, {why}", intent.id);
                report.submitted.push(entry(
                    intent,
                    Some(format!("Not attempted, {why}")),
                    false,
                    false,
                ));
                continue;
            }

            let (recorded, outcome) = match submit::deliver(&mut channel, outgoing) {
                Ok(()) => {
                    let copy = intent.copy().filter(|_| !files_copy);
                    // NOTE: both leave the intent's receipt with no seq: the
                    // copy is not in the store yet, a queued `add` of its
                    // own or one the provider files for the next sync
                    // (pimdir STORAGE §15.5).
                    let acknowledged = match &copy {
                        Some(collection) => {
                            let action = PimdirAction::Add {
                                link_id: None,
                                flags: PimdirFlags::from_iter(["\\Seen"]),
                                object: intent.object.clone(),
                            };
                            store.replace_action(intent.id, "neverest", collection, &action)
                        }
                        None => store.acknowledge_action(intent.id, None),
                    };
                    copies += usize::from(copy.is_some());
                    sent += 1;
                    let recorded = match acknowledged {
                        Ok(true) => true,
                        Ok(false) => {
                            warn!(
                                "submit intent #{} vanished from the queue before its acknowledgement",
                                intent.id
                            );
                            true
                        }
                        Err(err) => {
                            warn!(
                                "submit intent #{} was sent but could not be acknowledged, the next run parks it as uncertain: {err}",
                                intent.id
                            );
                            false
                        }
                    };
                    (recorded, entry(intent, None, false, false))
                }
                Err(failure) => {
                    let reason = failure.reason();
                    let (parked, uncertain) = (failure.parks(), failure.uncertain());
                    warn!("submit intent #{} failed: {reason}", intent.id);
                    // NOTE: the attempt is counted already, so a transient
                    // failure records nothing more.
                    let recorded = !parked
                        || match store.fail_action(intent.id, Some(&reason)) {
                            Ok(()) => true,
                            Err(err) => {
                                warn!("cannot park submit intent #{}: {err}", intent.id);
                                // NOTE: a refused message may be sent again
                                // and refused again; an uncertain one must
                                // not be, which the mark sees to.
                                !uncertain
                            }
                        };
                    (recorded, entry(intent, Some(reason), parked, uncertain))
                }
            };
            report.submitted.push(outcome);

            if recorded {
                if let Err(err) = in_flight.end() {
                    warn!(
                        "cannot clear the in-flight mark of submit intent #{}: {err}",
                        intent.id
                    );
                    halted = Some(format!("the in-flight mark could not be cleared: {err}"));
                }
            } else {
                halted = Some(String::from(
                    "an earlier message's outcome could not be recorded",
                ));
            }
        }

        channel.close();
        if sent > 0 {
            info!("submitted {sent} of {} queued intent(s)", intents.len());
        }
        if copies > 0 {
            drain_queues(store, report);
        }
    }

    #[cfg(not(any(feature = "smtp", feature = "msgraph")))]
    {
        let _ = (
            account_config,
            account,
            sides,
            blobs,
            store,
            report,
            files_copy,
        );
        warn!(
            "this build has no send channel (needs the `smtp` or the `msgraph` cargo feature), {} submit intent(s) stay pending",
            intents.len()
        );
    }
}

/// Parks the intent an earlier run left in flight as uncertain, and returns
/// the others, still to send.
///
/// That run was stopped between counting the attempt and recording its
/// outcome: the message may have left, so it is never sent again on its
/// own. `None` when the intent could not be parked, the run then sending
/// nothing, so the mark stays for the next.
fn park_interrupted(
    store: &mut PimdirSourceStore,
    in_flight: &submit::InFlight,
    mut intents: Vec<submit::SubmitIntent>,
    report: &mut SyncOutput,
) -> Option<Vec<submit::SubmitIntent>> {
    let Some(left) = in_flight.left() else {
        // NOTE: a mark is renamed into place whole, so one that does not
        // read names nothing.
        if let Err(err) = in_flight.end() {
            warn!("cannot clear an unreadable in-flight mark: {err}");
        }
        return Some(intents);
    };

    // NOTE: an intent no longer pending was acknowledged or parked before
    // the run stopped, its outcome recorded.
    if let Some(index) = intents.iter().position(|intent| intent.id == left) {
        let intent = intents.remove(index);
        let error = submit::uncertain_reason("a run stopped while handing it over");
        warn!("submit intent #{} parked: {error}", intent.id);
        if let Err(err) = store.fail_action(intent.id, Some(&error)) {
            warn!(
                "cannot park submit intent #{}, nothing is sent this run: {err}",
                intent.id
            );
            return None;
        }
        report.submitted.push(SubmitEntry {
            id: intent.id,
            collection: intent.collection.clone(),
            subject: intent.subject(),
            error: Some(error),
            parked: true,
            uncertain: true,
        });
    }

    if let Err(err) = in_flight.end() {
        warn!("cannot clear the in-flight mark, nothing is sent this run: {err}");
        return None;
    }
    Some(intents)
}

/// Performs the queued collection-create intents (pimdir STORAGE Annex
/// B.2), each by the side it names, and settles their rows.
///
/// It runs before the listing, so a collection created here is listed, and
/// synced, by the same run.
fn drain_collection_creates(
    sides: &mut [&mut SourceCtx],
    store: &mut PimdirSourceStore,
    report: &mut SyncOutput,
) {
    let intents = match create::pending(store) {
        Ok(intents) => intents,
        Err(err) => {
            warn!("cannot read the queued collection intents: {err:#}");
            return;
        }
    };

    for intent in &intents {
        // NOTE: an intent naming no source predates pimdir STORAGE §15.6,
        // and is performed by a side of its anchor's namespace.
        let index = match intent.source() {
            Some(name) => sides.iter().position(|ctx| ctx.name == name),
            None => sides.iter().position(|ctx| {
                intent
                    .collection
                    .starts_with(&format!("{}/", ctx.namespace))
            }),
        };
        let Some(index) = index else {
            continue;
        };

        let side = &mut *sides[index];
        let outcome = intent
            .request()
            .and_then(|request| perform_collection_create(side, &request));
        let row = QueuedIntent {
            id: intent.id,
            collection: &intent.collection,
            kind: COLLECTION_CREATE,
            seq: None,
        };
        let performer = side.name.clone();
        settle_intent(store, row, &performer, outcome, report);
    }
}

/// Creates the collection a collection-create asks for on `side`'s server.
fn perform_collection_create(
    side: &mut SourceCtx,
    request: &PimdirCollectionCreate,
) -> Result<(), InvitationFailure> {
    let prefix = format!("{}/", side.namespace);
    let parent = match &request.parent {
        Some(parent) => match parent.as_str().strip_prefix(&prefix) {
            Some(parent) => Some(parent.to_owned()),
            None => {
                return Err(InvitationFailure::Permanent(anyhow!(
                    "Parent {} is not a collection of source {}",
                    parent.as_str(),
                    side.name
                )));
            }
        },
        None => None,
    };

    let id = side
        .pool
        .primary()
        .create_named_collection(parent.as_deref(), &request.name)
        .map_err(InvitationFailure::classify)?;
    info!("collection {id} created on {}", side.name);
    Ok(())
}

/// Performs the queue's calendar intents, which the store's drain leaves
/// alone (pimdir STORAGE Annex B.2).
///
/// Each is performed by the side its payload names, on the item that side
/// binds, through the provider's own verbs. A performed intent is
/// acknowledged; a permanent failure parks the row with its error, a
/// transient one leaves it pending. An intent naming a source this run does
/// not drive waits for that source's own run.
fn drain_invitations(
    sides: &mut [&mut SourceCtx],
    store: &mut PimdirSourceStore,
    report: &mut SyncOutput,
) {
    let intents = match invitation::pending(store) {
        Ok(intents) => intents,
        Err(err) => {
            warn!("cannot read the queued calendar intents: {err:#}");
            return;
        }
    };
    if intents.is_empty() {
        return;
    }

    for intent in &intents {
        let located = locate_invitation(store, intent);
        // NOTE: an intent naming no source predates pimdir STORAGE §15.6,
        // and is performed by a side of its collection's namespace, the
        // one binding the item when two do.
        let index = match intent.source() {
            Some(name) => sides.iter().position(|ctx| ctx.name == name),
            None => {
                let candidates: Vec<usize> = (0..sides.len())
                    .filter(|&index| {
                        let prefix = format!("{}/", sides[index].namespace);
                        intent.collection.starts_with(&prefix)
                    })
                    .collect();
                let holder = candidates.iter().copied().find(|&index| {
                    located
                        .as_ref()
                        .is_ok_and(|bindings| bindings.contains_key(&source_id(&sides[index].name)))
                });
                holder.or(candidates.first().copied())
            }
        };
        let Some(index) = index else {
            continue;
        };

        let side = &mut *sides[index];
        let outcome = located.and_then(|bindings| perform_invitation(side, intent, &bindings));
        let performer = side.name.clone();
        settle_invitation(store, intent, &performer, outcome, report);
    }
}

/// Every source's binding of the item an intent addresses.
fn locate_invitation(
    store: &PimdirSourceStore,
    intent: &InvitationIntent,
) -> Result<BTreeMap<PimdirSourceId, PimdirBinding>, InvitationFailure> {
    let collection = &intent.collection;
    let transient = |err: PimdirError| {
        InvitationFailure::Transient(anyhow!("Cannot read the item from the store: {err}"))
    };

    let link = match intent.target()? {
        Target::Seq(seq) => match store.get_item(collection, seq).map_err(transient)? {
            Some(item) => item.link_id.0,
            None => {
                return Err(InvitationFailure::Permanent(anyhow!(
                    "Item #{seq} is gone from {collection}"
                )));
            }
        },
        // NOTE: the producer's own add, which the drain stages first.
        Target::Link(link) => match store.seq_for_link(collection, &link).map_err(transient)? {
            Some(_) => link,
            None => {
                return Err(InvitationFailure::Transient(anyhow!(
                    "No item of {collection} is keyed {link} yet"
                )));
            }
        },
    };

    store.item_bindings(collection, &link).map_err(transient)
}

/// Performs one intent on the item `side` binds.
fn perform_invitation(
    side: &mut SourceCtx,
    intent: &InvitationIntent,
    bindings: &BTreeMap<PimdirSourceId, PimdirBinding>,
) -> Result<(), InvitationFailure> {
    let invitation = intent.invitation()?;
    let Some(binding) = bindings.get(&source_id(&side.name)) else {
        return Err(InvitationFailure::Permanent(anyhow!(
            "Source {} does not hold the item {} addresses",
            side.name,
            intent.kind
        )));
    };
    // NOTE: a pending create holds a provisional handle until its add is
    // accepted, the next push (pimdir SYNC §4).
    let handle = binding.handle.as_str();
    if binding.base.is_none() || handle.starts_with('\u{1}') {
        return Err(InvitationFailure::Transient(anyhow!(
            "The item is not on {} yet, the intent waits for its push",
            side.name
        )));
    }

    let collection = wire_name(&side.namespace, &intent.collection).to_string();
    let handle = handle.to_string();
    let name = side.name.clone();
    let occurrence = intent.occurrence()?;

    // NOTE: an occurrence is acted on as its own instance, which the
    // provider's verbs take like any event.
    let outcome = match side.pool.primary() {
        #[cfg(feature = "gcal")]
        Client::Gcal(client) => {
            let target = match &occurrence {
                Some(recurrence_id) => client.occurrence(&collection, &handle, recurrence_id),
                None => Ok(handle.clone()),
            };
            target.and_then(|target| match invitation {
                Invitation::Reply { partstat, comment } => {
                    client.reply(&collection, &target, partstat, comment.as_deref())
                }
                Invitation::Cancel { .. } => client.cancel(&collection, &target),
            })
        }
        #[cfg(feature = "msgraph")]
        Client::Msgraph(client) if client.kind() == GraphKind::Calendar => {
            let target = match &occurrence {
                Some(recurrence_id) => client.occurrence_event(&handle, recurrence_id),
                None => Ok(handle.clone()),
            };
            target.and_then(|target| match invitation {
                Invitation::Reply { partstat, comment } => {
                    client.reply_event(&target, partstat, comment.as_deref())
                }
                Invitation::Cancel { comment } => client.cancel_event(&target, comment.as_deref()),
            })
        }
        #[allow(unreachable_patterns)]
        _ => {
            // NOTE: read here too, so a build performing no intent at all
            // still uses what the arms above read.
            let _: (&Invitation, &str, &str, &Option<String>) =
                (&invitation, &collection, &handle, &occurrence);
            Err(anyhow::Error::new(Refusal(format!(
                "Source {name} cannot perform {}",
                intent.kind
            ))))
        }
    };

    outcome.map_err(InvitationFailure::classify)
}

/// Acknowledges or fails one performed invitation intent, and reports it.
fn settle_invitation(
    store: &mut PimdirSourceStore,
    intent: &InvitationIntent,
    performer: &str,
    outcome: Result<(), InvitationFailure>,
    report: &mut SyncOutput,
) {
    let row = QueuedIntent {
        id: intent.id,
        collection: &intent.collection,
        kind: &intent.kind,
        seq: intent.seq(),
    };
    settle_intent(store, row, performer, outcome, report);
}

/// The queue row of an intent neverest performs, as it is settled and
/// reported.
struct QueuedIntent<'a> {
    id: i64,
    collection: &'a str,
    kind: &'a str,
    seq: Option<i64>,
}

/// Acknowledges or fails one performed intent, and reports it.
///
/// A performed intent is acknowledged with its receipt, so a producer reads it
/// applied rather than withdrawn (pimdir STORAGE §15.4). The receipt names no
/// item: a created collection arrives with a later listing, and an invitation
/// answered or cancelled leaves its event where the producer found it.
fn settle_intent(
    store: &mut PimdirSourceStore,
    intent: QueuedIntent<'_>,
    performer: &str,
    outcome: Result<(), InvitationFailure>,
    report: &mut SyncOutput,
) {
    let (error, parked) = match outcome {
        Ok(()) => {
            info!(
                "{} intent #{} performed by {performer}",
                intent.kind, intent.id
            );
            match store.acknowledge_action(intent.id, None) {
                Ok(true) => {}
                Ok(false) => warn!(
                    "{} intent #{} vanished from the queue before its acknowledgement",
                    intent.kind, intent.id
                ),
                Err(err) => warn!(
                    "{} intent #{} was performed but could not be acknowledged, it will be performed again: {err}",
                    intent.kind, intent.id
                ),
            }
            (None, false)
        }
        Err(failure) => {
            let error = format!("{:#}", failure.error());
            let parked = failure.parks();
            warn!("{} intent #{} failed: {error}", intent.kind, intent.id);
            if let Err(err) = store.fail_action(intent.id, parked.then_some(error.as_str())) {
                warn!(
                    "cannot record {} intent #{} failure: {err}",
                    intent.kind, intent.id
                );
            }
            (Some(error), parked)
        }
    };

    report.intents.push(IntentEntry {
        id: intent.id,
        collection: intent.collection.to_owned(),
        kind: intent.kind.to_owned(),
        source: performer.to_string(),
        seq: intent.seq,
        error,
        parked,
    });
}

/// Reclaims the retained items older than `store.purge-after`, unset never.
///
/// It runs after the sync, so an item this run retired starts its delay rather
/// than going with it, and it warns instead of failing. A purge only releases a
/// body (pimdir SPEC §5), so the collector runs here too, after a purge only.
fn sweep_retained(
    account_config: &AccountConfig,
    store: &mut PimdirStore,
    report: &mut SyncOutput,
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

/// The source whose channel a run over `sides` sends through, found as
/// [`open_send_channel`] picks it.
fn sending_source(account_config: &AccountConfig, sides: &[SendSide<'_>]) -> Option<String> {
    let configured = account_config.sources().ok()?;
    sides
        .iter()
        .find(|side| {
            configured
                .get(side.name)
                .is_some_and(|source| source.smtp.is_some() || source.sends_natively())
        })
        .map(|side| side.name.to_owned())
}

/// Why a run has no send channel for its intents.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
enum Unopened {
    /// None this build or this account offers: the intents stay pending,
    /// unreported, another build or run being able to send them.
    Pending,
    /// The SMTP server could not be reached: named under `unreached`, the
    /// intents pending and unattempted.
    #[cfg_attr(not(feature = "smtp"), allow(dead_code))]
    Unreached(UnreachedEndpoint),
    /// The SMTP server refused the credentials: each intent fails with this
    /// error, pending and unattempted.
    #[cfg_attr(not(feature = "smtp"), allow(dead_code))]
    Refused(String),
}

/// Resolves the account's send channel: an `smtp` table, else a native send.
///
/// At most one source may declare an `smtp` table, the account refusing more at
/// load, so there is no tiebreak here. A channel the run cannot have warns and
/// leaves the intents pending.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
#[cfg_attr(not(feature = "smtp"), allow(unused_variables))]
fn open_send_channel<'a>(
    account_config: &AccountConfig,
    account: &Account,
    sides: &'a mut [SendSide<'_>],
    queued: usize,
) -> Result<submit::SendChannel<'a>, Unopened> {
    let configured = account_config.sources().map_err(|err| {
        warn!("cannot read the account's sources, {queued} intent(s) stay pending: {err:#}");
        Unopened::Pending
    })?;
    let pick = sides.iter().enumerate().find_map(|(index, side)| {
        let source = configured.get(side.name)?;
        match &source.smtp {
            Some(_) => Some(SendChannelPick::Smtp(side.name.to_owned())),
            None if source.sends_natively() => Some(SendChannelPick::Native(index)),
            None => None,
        }
    });

    match pick {
        #[cfg(feature = "smtp")]
        Some(SendChannelPick::Smtp(name)) => {
            let source = account.get(&name).map_err(|err| {
                warn!("cannot resolve the smtp channel, {queued} intent(s) stay pending: {err:#}");
                Unopened::Pending
            })?;
            let smtp = source.smtp.expect("the pick found a configured channel");
            match submit::connect_smtp(&smtp) {
                Ok(client) => Ok(submit::SendChannel::smtp(client)),
                Err(err) => {
                    let error = format!("{err:#}");
                    warn!("cannot open the smtp channel, {queued} intent(s) stay pending: {error}");
                    match submit::open_failure(&err) {
                        submit::OpenFailure::Unreached => {
                            Err(Unopened::Unreached(UnreachedEndpoint {
                                endpoint: format!("{name}.smtp"),
                                error,
                            }))
                        }
                        submit::OpenFailure::Refused => Err(Unopened::Refused(error)),
                    }
                }
            }
        }
        #[cfg(not(feature = "smtp"))]
        Some(SendChannelPick::Smtp(_)) => {
            warn!(
                "the source's `smtp` table needs the `smtp` cargo feature, {queued} intent(s) stay pending"
            );
            Err(Unopened::Pending)
        }
        #[cfg(feature = "msgraph")]
        Some(SendChannelPick::Native(index)) => match sides[index].client.as_deref_mut() {
            Some(Client::Msgraph(client)) if client.kind() == GraphKind::Mail => {
                Ok(submit::SendChannel::graph(client.as_mut()))
            }
            #[allow(unreachable_patterns)]
            _ => {
                warn!("no send channel available, {queued} intent(s) stay pending");
                Err(Unopened::Pending)
            }
        },
        #[cfg(not(feature = "msgraph"))]
        Some(SendChannelPick::Native(_)) => {
            warn!(
                "the side sends through Microsoft Graph, which needs the `msgraph` cargo feature, {queued} intent(s) stay pending"
            );
            Err(Unopened::Pending)
        }
        None => {
            warn!("no send channel available, {queued} intent(s) stay pending");
            Err(Unopened::Pending)
        }
    }
}

/// Which source performs the submit intents, and how.
///
/// Each variant is consumed by the arm its cargo feature gates, so a build with
/// only one of them carries the other unread rather than duplicating the walk
/// per feature combination.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
#[allow(dead_code)]
enum SendChannelPick {
    /// The name of the source whose `smtp` table the intents leave through.
    Smtp(String),
    /// The index of the source that sends by itself.
    Native(usize),
}

/// Hydrates every not-yet-`Full` placement of both sides.
///
/// The store then mirrors every body, and the shared object store dedups, so
/// the second side's upgrade links an already-stored body by link id without a
/// fetch.
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
        let targets: Vec<PimdirHandle> = load_side(store, collection)
            .with_context(|| format!("Load {} {collection}", &ctx.name))?
            .into_iter()
            .filter(|p| p.status != PimdirStatus::Tombstone && p.level < PimdirLevel::Full)
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
        run_verb(
            store,
            &mut remote,
            PimdirUpgrade::new(collection.to_string(), targets, PimdirTier::Full),
        )
        .with_context(|| format!("Hydrate all bodies {} {collection}", &ctx.name))?;
        raised += total;
    }
    Ok(raised)
}

/// The blob subdirectory of a pimdir store (SPEC §5), shared by a dry run.
const BLOBS_DIR: &str = "objects";

/// A throwaway replica of the pimdir store, removed however the run ends.
///
/// Built beside the real store, not under the temporary directory, so the two
/// share a filesystem and the bodies are hardlinked rather than copied, which
/// on a tmpfs also saves gigabytes. Only the blob tree is ever shared.
struct DryRunReplica {
    /// Where the replica lives, which is the run's `work_dir`.
    dir: PathBuf,
}

impl DryRunReplica {
    /// Clones the store at `real_dir` into a sibling directory.
    ///
    /// Whatever an earlier run left behind is cleared first: a panic aborts a
    /// release build, running no destructor, so a leftover is a state this has
    /// to meet rather than one it can rule out.
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

            // NOTE: a blob tree that could not be shared was read and written
            // whole, the slow dry run this exists to avoid.
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
///
/// A directory that will not go is left with a warning: it costs disk, never
/// correctness, the run about to start writing under a name of its own.
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

/// Clones `src` into `dst`, hardlinking under `blobs` and copying the rest.
///
/// A link the filesystem refuses falls back to a copy, so the cost of
/// misjudging a file is a slower dry run and never a corrupted store.
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
    use io_pimdir::{
        change::{PimdirChange, PimdirChangeKind, PimdirDropReason},
        client::reader::PimdirReader,
        collection::{PimdirCheckpoint, PimdirCursor},
        object::PimdirObject,
        placement::PimdirOrigin,
        remote::{
            PimdirEnumerate, PimdirEnumerated, PimdirFetchedBody, PimdirListing, PimdirPushOutcome,
            PimdirPushResult, PimdirRemote, PimdirRemoteItem, PimdirRemoteMeta,
            PimdirRemoteSnapshot,
        },
        summary::mail::PimdirMailSummary,
    };

    use super::*;

    /// What names a listed member by `link` alone, as a store binding does.
    fn named(link: &str) -> PimdirRemoteMeta {
        PimdirRemoteMeta {
            link_id: PimdirLinkId(link.into()),
            summary: None,
            sort_key: PimdirSortKey::default(),
            body: None,
        }
    }

    /// The marker survives the context a caller adds on top of the open,
    /// and a failure that is not an open carries none.
    #[test]
    fn an_open_failure_reads_as_unreached_through_later_context() {
        let refused = anyhow!("connect 127.0.0.1:1: Connection refused")
            .context(Unreached::new("imap", "Open connection"))
            .context("Sync source imap");
        let unreached = refused.downcast_ref::<Unreached>().expect("marked");
        assert_eq!(unreached.endpoint, "imap");
        assert_eq!(
            format!("{refused:#}"),
            "Sync source imap: Open connection: connect 127.0.0.1:1: Connection refused"
        );

        let unreadable = anyhow!("NO Mailbox does not exist").context("Select INBOX");
        assert!(unreadable.downcast_ref::<Unreached>().is_none());
    }
    use crate::{cli::exit::Exit, offline::source_id};

    fn listed(roles: &[(&str, Option<&str>)]) -> Vec<Collection> {
        roles
            .iter()
            .map(|(id, role)| Collection {
                id: id.to_string(),
                name: id.to_string(),
                total: None,
                unread: None,
                role: role.map(String::from),
            })
            .collect()
    }

    /// A listing carrying its roles asks nothing more, and a lookup the
    /// listing needs is asked once per collection set, reused while the
    /// set holds, asked again when it moves.
    #[test]
    fn roles_a_listing_lacks_are_looked_up_once_per_collection_set() {
        let mut state = StoreState::default();
        let carried = listed(&[
            ("INBOX", Some("inbox")),
            ("Sent", Some("sent")),
            ("Work", None),
        ]);
        let roles = stated_roles(|| Ok(None), "imap", &carried, &mut state).unwrap();
        assert_eq!(roles.len(), 2);
        assert_eq!(roles["Sent"], "sent");
        assert!(state.looked_up_roles.is_empty(), "nothing to keep");

        let folders = listed(&[("AAA", None), ("BBB", None)]);
        let found = || Ok(Some(BTreeMap::from([("BBB".into(), "sent".into())])));
        let roles = stated_roles(found, "graph", &folders, &mut state).unwrap();
        assert_eq!(roles["BBB"], "sent");

        let roles = stated_roles(
            || panic!("the same set is not looked up again"),
            "graph",
            &folders,
            &mut state,
        )
        .unwrap();
        assert_eq!(roles["BBB"], "sent", "the kept answer");

        let moved = listed(&[("AAA", None), ("BBB", None), ("CCC", None)]);
        let found = || Ok(Some(BTreeMap::from([("CCC".into(), "sent".into())])));
        let roles = stated_roles(found, "graph", &moved, &mut state).unwrap();
        assert_eq!(roles.get("BBB"), None, "the server moved its role");
        assert_eq!(roles["CCC"], "sent");

        let failed = || Err(anyhow!("offline"));
        let more = listed(&[("DDD", None)]);
        assert!(
            stated_roles(failed, "graph", &more, &mut state).is_none(),
            "a failed lookup clears nothing"
        );
    }

    /// The layout below the platform base is one shape everywhere, so a store
    /// stays addressable by the same relative path whichever platform wrote
    /// it, and `store.root` still overrides the whole thing.
    #[test]
    fn the_default_replica_root_is_the_account_under_a_platform_base() {
        let dir = replica_dir("work").unwrap();

        assert!(dir.ends_with("neverest/work"), "{}", dir.display());
        assert!(dir.is_absolute(), "{}", dir.display());

        let mut config = AccountConfig::default();
        config.store.root = Some(PathBuf::from("/tmp/elsewhere"));

        assert_eq!(
            store_dir("work", &config).unwrap(),
            PathBuf::from("/tmp/elsewhere"),
        );
    }

    /// A store with one body and the files a dry run writes to.
    fn stub_store(dir: &Path) {
        let body = dir.join(BLOBS_DIR).join("ab").join("cd");
        fs::create_dir_all(&body).unwrap();
        fs::write(body.join("abcd"), b"body").unwrap();
        fs::write(dir.join("pimdir.db"), b"index").unwrap();
        fs::write(dir.join("neverest.json"), b"{}").unwrap();
    }

    /// The rule the replica rests on.
    ///
    /// A body is shared, so a mail account's blob tree is not read and written
    /// whole, and everything a dry run writes to is its own, so those writes
    /// cannot reach the real store.
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

    /// Writing to the replica's index leaves the real one alone.
    ///
    /// That is what a dry run means, and what a shared index would break.
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

    /// The replica goes however the run ends, an early return included.
    ///
    /// Which is why it is a guard rather than a line on the way out.
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

    /// A release build aborts on a panic and runs no destructor.
    ///
    /// The next run is what clears what the aborted one left behind.
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

    /// An action is applied as the source syncing its collection.
    ///
    /// On an account syncing three kinds, `caldav` sorts first and reaches
    /// every mail action before `imap` does; the store now hands the action
    /// to `imap` under `caldav`'s handle.
    #[test]
    fn a_drain_applies_an_action_as_the_source_of_its_collection() {
        let dir = tempfile::tempdir().unwrap();
        let mut imap = PimdirStore::open(dir.path()).unwrap().for_source("imap");
        imap.ensure_collection("imap/INBOX", "message/rfc822")
            .unwrap();

        let mut producer = PimdirProducer::open(dir.path(), "himalaya").unwrap();
        let queue = |producer: &mut PimdirProducer, link: &str| {
            producer
                .enqueue(
                    "imap/INBOX",
                    &PimdirAction::Add {
                        link_id: Some(PimdirLinkId(link.into())),
                        flags: PimdirFlags::default(),
                        object: None,
                    },
                    None,
                )
                .unwrap();
        };

        queue(&mut producer, "mid:first@x");
        let mut report = SyncOutput::default();
        drain_queues(&mut imap, &mut report);
        assert_eq!(report.drained.len(), 1);
        assert_eq!(report.drained[0].collection, "imap/INBOX");
        assert_eq!(report.drained[0].applied, 1);

        queue(&mut producer, "mid:second@x");
        let mut caldav = PimdirStore::open(dir.path()).unwrap().for_source("caldav");
        let mut report = SyncOutput::default();
        drain_queues(&mut caldav, &mut report);
        assert_eq!(report.drained.len(), 1);
        assert_eq!(report.drained[0].collection, "imap/INBOX");
        assert_eq!(report.drained[0].applied, 1);
        assert_eq!(
            caldav.collection_sources("imap/INBOX").unwrap(),
            vec!["imap".to_string()],
            "the create is imap's to push"
        );
    }

    /// A parked row belongs to the store, and is reported once however many
    /// drains ran: reading the parked rows where the drain runs reported one
    /// row per drain, so an account syncing three kinds showed the same warning
    /// three times.
    #[test]
    fn a_parked_action_is_reported_once_however_many_drains_ran() {
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
                    flags: PimdirFlags::from_iter(["\\Seen"]),
                },
                None,
            )
            .unwrap();

        let mut report = SyncOutput::default();
        drain_queues(&mut store, &mut report);
        drain_queues(&mut store, &mut report);
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
        let hash = PimdirHash("cafe0001".into());
        let object = PimdirObject {
            hash: hash.clone(),
            size: writer.commit(&hash).unwrap() as usize,
        };

        let mut producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
        producer
            .enqueue(
                "left/INBOX",
                &PimdirAction::Add {
                    link_id: Some(PimdirLinkId("mid:q1@x".into())),
                    flags: PimdirFlags::from_iter(["\\Seen"]),
                    object: Some(hash),
                },
                Some(&object),
            )
            .unwrap();
        producer
            .enqueue("left/INBOX", &PimdirAction::Remove { seq: 424242 }, None)
            .unwrap();
        producer
            .enqueue(
                "left/INBOX",
                &PimdirAction::SetFlags {
                    seq: 424243,
                    flags: PimdirFlags::from_iter(["\\Seen"]),
                },
                None,
            )
            .unwrap();

        let mut report = SyncOutput::default();
        drain_queues(&mut store, &mut report);
        report_parked(&store, &mut report);

        assert_eq!(report.drained.len(), 1);
        assert_eq!(report.drained[0].collection, "left/INBOX");
        assert_eq!(report.drained[0].applied, 2);
        assert_eq!(report.parked.len(), 1);
        assert!(report.parked[0].error.contains("unknown seq"));

        let placements = load_side(&store, "left/INBOX").unwrap();
        assert_eq!(placements.len(), 1);
        // NOTE: a drained `Add` is a create the next sync owes the source. It
        // read `Dirty` until the engine's a-bound-create-is-still-a-create,
        // the shape that stranded every queued create.
        assert_eq!(placements[0].status, PimdirStatus::Created);
        assert!(placements[0].base.is_none());
        assert_eq!(
            placements[0].link_id.as_ref().map(|l| l.0.as_str()),
            Some("mid:q1@x")
        );

        let mut second = SyncOutput::default();
        drain_queues(&mut store, &mut second);
        report_parked(&store, &mut second);
        assert!(second.drained.is_empty());
        assert_eq!(second.parked.len(), 1);
    }

    /// The stored checkpoint's UIDVALIDITY, as the IMAP adapter encodes it.
    ///
    /// The guard itself goes through `Client::handle_space_epoch`, but this
    /// test has no client, which is also why it is gated on the IMAP feature.
    #[cfg(feature = "imap")]
    fn stored_checkpoint_uid_validity(store: &PimdirSourceStore, collection: &str) -> Option<u32> {
        let loaded = store
            .load(
                &PimdirCollectionId(collection.to_string()),
                &PimdirLoadScope::All,
            )
            .unwrap();
        loaded
            .checkpoint
            .as_ref()
            .and_then(|checkpoint| crate::imap::backend::checkpoint_uid_validity(&checkpoint.0))
    }

    /// A scripted remote for the rekey pump: a fixed spine, rejecting pushes.
    #[cfg(feature = "imap")]
    struct ScriptedRemote {
        /// The new handle space: `(handle, link id)` pairs.
        spine: Vec<(String, String)>,
    }

    #[cfg(feature = "imap")]
    impl PimdirRemote for ScriptedRemote {
        type Error = anyhow::Error;

        fn enumerate(
            &mut self,
            _collection: &PimdirCollectionId,
            _request: PimdirEnumerate,
        ) -> Result<PimdirEnumerated, Self::Error> {
            let items = self
                .spine
                .iter()
                .map(|(handle, link)| PimdirRemoteItem {
                    handle: PimdirHandle(handle.clone()),
                    flags: PimdirFlags::default(),
                    revision: None,
                    meta: named(link),
                })
                .collect();
            Ok(PimdirEnumerated::Page(PimdirRemoteSnapshot::round(
                items,
                Some(PimdirCheckpoint(crate::imap::backend::encode_checkpoint(
                    2, 1,
                ))),
            )))
        }

        fn fetch(
            &mut self,
            _collection: &PimdirCollectionId,
            handles: Vec<PimdirHandle>,
            _tier: PimdirTier,
        ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
            Ok(handles
                .into_iter()
                .filter_map(|handle| {
                    let link = self
                        .spine
                        .iter()
                        .find(|(h, _)| *h == handle.0)
                        .map(|(_, link)| link.clone())?;
                    Some(PimdirFetchedItem {
                        handle,
                        link_id: PimdirLinkId(link),
                        summary: None,
                        sort_key: PimdirSortKey::default(),
                        body: None,
                        revision: None,
                    })
                })
                .collect())
        }

        fn push(
            &mut self,
            _collection: &PimdirCollectionId,
            _changes: Vec<PimdirChange>,
        ) -> Result<Vec<PimdirPushResult>, Self::Error> {
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
                PimdirWriteOp::StoreObject {
                    object: PimdirObject {
                        hash: PimdirHash("beef0001".into()),
                        size: 3,
                    },
                    body: Some(b"abc".to_vec()),
                },
                PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                    collection: PimdirCollectionId("INBOX".into()),
                    handle: PimdirHandle("1".into()),
                    link_id: Some(PimdirLinkId("mid:a@x".into())),
                    object: Some(PimdirHash("beef0001".into())),
                    level: PimdirLevel::Full,
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    flags: PimdirFlags::default(),
                    status: PimdirStatus::Clean,
                    conflict_revision: None,
                    conflict_object: None,
                    base: Some(PimdirBase {
                        flags: PimdirFlags::default(),
                        revision: None,
                        object: Some(PimdirHash("beef0001".into())),
                    }),
                    origin: None,
                }),
                PimdirWriteOp::SetCheckpoint {
                    collection: PimdirCollectionId("INBOX".into()),
                    checkpoint: PimdirCheckpoint(crate::imap::backend::encode_checkpoint(1, 1)),
                },
            ])
            .unwrap();
        assert_eq!(store.generation("INBOX").unwrap(), Some(1));

        let mut remote = ScriptedRemote {
            spine: vec![(String::from("7"), String::from("mid:a@x"))],
        };
        let rekey = run_verb(&mut store, &mut remote, PimdirRekey::new("INBOX")).unwrap();

        assert_eq!(rekey.rekeyed, 1);
        assert_eq!(rekey.pulled, 0);
        assert_eq!(store.generation("INBOX").unwrap(), Some(2));

        let placements = load_side(&store, "INBOX").unwrap();
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].handle.0, "7");
        assert_eq!(placements[0].object, Some(PimdirHash("beef0001".into())));
        assert_eq!(placements[0].status, PimdirStatus::Clean);

        assert_eq!(stored_checkpoint_uid_validity(&store, "INBOX"), Some(2));
    }

    fn names(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    /// The hunks as `(kind, side, collection)`, the hunk type having no
    /// equality of its own.
    fn hunk_list(hunks: &[CollectionHunk]) -> Vec<(&'static str, String, String)> {
        hunks
            .iter()
            .map(|hunk| match hunk {
                CollectionHunk::Create { side, collection } => {
                    ("create", side.clone(), collection.clone())
                }
                CollectionHunk::Delete { side, collection } => {
                    ("delete", side.clone(), collection.clone())
                }
                CollectionHunk::Scan { .. } => ("scan", String::new(), String::new()),
            })
            .collect()
    }

    const GRANTED: CollectionPermissions = CollectionPermissions {
        create: true,
        delete: true,
    };

    #[test]
    fn a_collection_new_on_one_side_is_created_on_the_other() {
        let hunks = diff_collections(
            &names(&["both", "new-a"]),
            &names(&["both", "new-b"]),
            &names(&[]),
            ("a", &GRANTED),
            ("b", &GRANTED),
        );

        assert_eq!(
            hunk_list(&hunks),
            vec![
                ("create", "b".into(), "new-a".into()),
                ("create", "a".into(), "new-b".into()),
            ]
        );
    }

    #[test]
    fn a_collection_both_held_and_one_deleted_is_deleted_on_the_other() {
        let hunks = diff_collections(
            &names(&["kept", "gone-on-b"]),
            &names(&["kept", "gone-on-a"]),
            &names(&["gone-on-a", "gone-on-b"]),
            ("a", &GRANTED),
            ("b", &GRANTED),
        );

        assert_eq!(
            hunk_list(&hunks),
            vec![
                ("delete", "a".into(), "gone-on-b".into()),
                ("delete", "b".into(), "gone-on-a".into()),
            ]
        );
    }

    #[test]
    fn a_delete_the_remaining_side_forbids_is_not_undone_by_a_create() {
        let keep = CollectionPermissions {
            create: true,
            delete: false,
        };
        let hunks = diff_collections(
            &names(&["gone-on-b"]),
            &names(&[]),
            &names(&["gone-on-b"]),
            ("a", &keep),
            ("b", &GRANTED),
        );

        assert!(hunks.is_empty(), "{hunks:?}");
    }

    #[test]
    fn no_collection_name_is_reserved() {
        let collections: BTreeMap<String, String> = ["INBOX", "Outbox", "Sent"]
            .map(|name| (String::from(name), String::from(name)))
            .into();

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
    fn a_collection_is_named_without_the_namespace_that_addresses_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = PimdirStore::open(dir.path()).unwrap().for_source("caldav");
        let read = || {
            PimdirReader::open(dir.path())
                .unwrap()
                .list_collections()
                .unwrap()
        };

        // NOTE: what a CalDAV server answers, the id being the path
        // segment and the name the `DAV:displayname` beside it.
        let dav = BTreeMap::from([(String::from("ED99C7C8"), String::from("Work"))]);
        store
            .ensure_collection("caldav/ED99C7C8", "text/calendar")
            .unwrap();
        declare_name(&store, "caldav/ED99C7C8", "ED99C7C8", &[&dav]);

        let named = &read()[0];
        assert_eq!(named.id, "caldav/ED99C7C8", "the address keeps its prefix");
        assert_eq!(named.name, "Work", "the label carries neither");

        // NOTE: a source with nothing to say leaves the bare id, which is
        // the mailbox name on IMAP and the path segment on DAV.
        let silent = BTreeMap::from([(String::from("Archives"), String::new())]);
        store
            .ensure_collection("caldav/Archives", "text/calendar")
            .unwrap();
        declare_name(&store, "caldav/Archives", "Archives", &[&silent, &dav]);

        let fallback = read().into_iter().find(|c| c.id.ends_with("Archives"));
        assert_eq!(fallback.unwrap().name, "Archives");
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
        let hash = PimdirHash("cafe0002".into());
        let object = PimdirObject {
            hash: hash.clone(),
            size: writer.commit(&hash).unwrap() as usize,
        };

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
                Some(&object),
            )
            .unwrap();

        let mut report = SyncOutput::default();
        drain_queues(&mut store, &mut report);
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

    /// A store whose `gcal` source declares the intents on `gcal/primary`,
    /// with one `calendar-reply` queued there naming `target`.
    fn queued_reply(dir: &Path, target: &str) -> PimdirSourceStore {
        let mut store = PimdirStore::open(dir)
            .unwrap()
            .for_account("live")
            .for_source("gcal");
        store
            .ensure_collection("gcal/primary", "text/calendar")
            .unwrap();
        let config: AccountConfig = toml::from_str(r#"gcal.auth.token.raw = "token""#).unwrap();
        let source = config.sources().unwrap().remove("gcal").unwrap();
        let declared = capability::declaration(&source, false, &["gcal/primary".into()]);
        store.declare("gcal", &declared).unwrap();

        PimdirProducer::open(dir, "test-frontend")
            .unwrap()
            .enqueue(
                "gcal/primary",
                &PimdirAction::Unknown {
                    kind: invitation::REPLY.into(),
                    payload: format!(r#"{{"v":1,"source":"gcal",{target},"partstat":"ACCEPTED"}}"#),
                    object_hash: None,
                },
                None,
            )
            .unwrap();
        store
    }

    #[test]
    fn a_calendar_intent_survives_the_drain_where_its_source_performs_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = queued_reply(dir.path(), r#""seq":1"#);

        let mut report = SyncOutput::default();
        drain_queues(&mut store, &mut report);
        assert!(report.parked.is_empty());

        let intents = invitation::pending(&store).unwrap();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].collection, "gcal/primary");
        assert_eq!(intents[0].source().as_deref(), Some("gcal"));
    }

    #[test]
    fn an_intent_on_a_gone_item_parks_and_one_on_a_pending_add_waits() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = queued_reply(dir.path(), r#""seq":42"#);
        let intent = invitation::pending(&store).unwrap().remove(0);

        let failure = locate_invitation(&store, &intent).unwrap_err();
        assert!(failure.parks(), "{:#}", failure.error());

        let mut report = SyncOutput::default();
        settle_invitation(&mut store, &intent, "gcal", Err(failure), &mut report);
        assert!(invitation::pending(&store).unwrap().is_empty());
        assert_eq!(store.parked_actions().unwrap().len(), 1);
        assert!(report.intents[0].parked);
        assert_eq!(report.intents[0].seq, Some(42));

        let dir = tempfile::tempdir().unwrap();
        let store = queued_reply(dir.path(), r#""link_id":"uid:not-yet""#);
        let intent = invitation::pending(&store).unwrap().remove(0);
        assert!(!locate_invitation(&store, &intent).unwrap_err().parks());
    }

    /// A performed intent leaves a receipt, so its producer reads it applied
    /// rather than withdrawn (pimdir STORAGE §15.4, §15.5).
    #[test]
    fn a_performed_intent_reads_as_applied_by_its_producer() {
        use io_pimdir::client::producer::PimdirActionStatus;

        let dir = tempfile::tempdir().unwrap();
        let mut store = queued_reply(dir.path(), r#""seq":1"#);
        let intent = invitation::pending(&store).unwrap().remove(0);

        let mut report = SyncOutput::default();
        settle_invitation(&mut store, &intent, "gcal", Ok(()), &mut report);
        assert!(invitation::pending(&store).unwrap().is_empty());
        assert!(report.intents[0].error.is_none());

        let producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
        let PimdirActionStatus::Applied {
            collection, seq, ..
        } = producer.action_status(intent.id).unwrap()
        else {
            panic!("a performed intent leaves its receipt");
        };
        assert_eq!(collection, "gcal/primary");
        assert_eq!(seq, None);
    }

    #[test]
    fn the_retention_sweep_runs_only_when_a_delay_is_configured() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
        let mut config: AccountConfig =
            toml::from_str(r#"left.imap.server = "imaps://imap.example.org:993""#).unwrap();

        let mut report = SyncOutput::default();
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
    /// collects nothing by itself (pimdir SPEC §5). Without a collector here a
    /// backup store grows without bound while reporting reclaimed bytes.
    #[test]
    fn the_sweep_collects_the_bodies_the_purge_released() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = PimdirStore::open(dir.path()).unwrap().for_source("left");

        source
            .write(vec![
                PimdirWriteOp::StoreObject {
                    object: PimdirObject {
                        hash: PimdirHash("beef0002".into()),
                        size: 3,
                    },
                    body: Some(b"abc".to_vec()),
                },
                PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                    collection: PimdirCollectionId("INBOX".into()),
                    handle: PimdirHandle("1".into()),
                    link_id: Some(PimdirLinkId("mid:gone@x".into())),
                    object: Some(PimdirHash("beef0002".into())),
                    level: PimdirLevel::Full,
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    flags: PimdirFlags::default(),
                    status: PimdirStatus::Clean,
                    conflict_revision: None,
                    conflict_object: None,
                    base: Some(PimdirBase {
                        flags: PimdirFlags::default(),
                        revision: None,
                        object: Some(PimdirHash("beef0002".into())),
                    }),
                    origin: None,
                }),
            ])
            .unwrap();

        let body = source.blobs().path(&PimdirHash("beef0002".into()));
        assert!(body.is_file(), "the body was stored");

        source
            .write(vec![PimdirWriteOp::DropPlacement {
                collection: PimdirCollectionId("INBOX".into()),
                handle: PimdirHandle("1".into()),
                reason: PimdirDropReason::Deleted,
            }])
            .unwrap();
        assert!(body.is_file(), "retention keeps the body");

        // NOTE: a `0s` delay purges what was retained strictly before a cutoff
        // carrying milliseconds, so an item dropped and swept within one of
        // them is not old enough to go.
        thread::sleep(std::time::Duration::from_millis(5));

        let mut store = PimdirStore::open(dir.path()).unwrap();
        let config: AccountConfig = toml::from_str(
            r#"
            left.imap.server = "imaps://imap.example.org:993"
            store.purge-after = "0s"
            "#,
        )
        .unwrap();

        let mut report = SyncOutput::default();
        drop(source);
        sweep_retained(&config, &mut store, &mut report);

        let purged = report.purged.expect("a retention section");
        assert_eq!(purged.items, 1);
        assert_eq!(purged.objects, 1);
        assert_eq!(purged.bytes, 3);
        assert!(!body.exists(), "the body is gone from the blob tree");
    }

    /// A mutable-content remote: every item carries an ETag.
    ///
    /// The path no compiled-in backend exercises yet, proven here before any
    /// DAV code exists, so a CardDAV failure later is a protocol bug rather
    /// than an engine one.
    struct MutableRemote {
        /// `handle -> (revision, accepted body)`.
        items: HashMap<String, (String, Option<PimdirHash>)>,
        /// The revision handed out by the next accepted write.
        next_revision: String,
        /// Every change this remote was asked to push, in order.
        pushed: Vec<PimdirChange>,
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

    impl PimdirRemote for MutableRemote {
        type Error = anyhow::Error;

        fn enumerate(
            &mut self,
            _collection: &PimdirCollectionId,
            _request: PimdirEnumerate,
        ) -> Result<PimdirEnumerated, Self::Error> {
            let items = self
                .items
                .iter()
                .map(|(handle, (revision, _))| PimdirRemoteItem {
                    handle: PimdirHandle(handle.clone()),
                    flags: PimdirFlags::default(),
                    revision: Some(revision.clone()),
                    meta: named("uid:a"),
                })
                .collect();
            Ok(PimdirEnumerated::Page(PimdirRemoteSnapshot::round(
                items,
                Some(PimdirCheckpoint(b"token-1".to_vec())),
            )))
        }

        fn fetch(
            &mut self,
            _collection: &PimdirCollectionId,
            handles: Vec<PimdirHandle>,
            _tier: PimdirTier,
        ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
            Ok(handles
                .into_iter()
                .filter_map(|handle| {
                    let (revision, _) = self.items.get(&handle.0)?;
                    Some(PimdirFetchedItem {
                        handle,
                        link_id: PimdirLinkId("uid:a".into()),
                        summary: None,
                        sort_key: PimdirSortKey::default(),
                        body: None,
                        revision: Some(revision.clone()),
                    })
                })
                .collect())
        }

        fn push(
            &mut self,
            _collection: &PimdirCollectionId,
            changes: Vec<PimdirChange>,
        ) -> Result<Vec<PimdirPushResult>, Self::Error> {
            let mut results = Vec::new();
            for change in changes {
                self.pushed.push(change.clone());
                let result = match &change.kind {
                    PimdirChangeKind::Update {
                        handle,
                        object,
                        if_match,
                    } => {
                        let current = self.items.get(&handle.0).map(|(rev, _)| rev.clone());
                        if if_match.is_some() && if_match.as_deref() == current.as_deref() {
                            let revision = self.next_revision.clone();
                            self.items
                                .insert(handle.0.clone(), (revision.clone(), Some(object.clone())));
                            PimdirPushResult {
                                handle: handle.clone(),
                                outcome: PimdirPushOutcome::Accepted,
                                assigned: None,
                                revision: Some(revision),
                            }
                        } else {
                            PimdirPushResult {
                                handle: handle.clone(),
                                outcome: PimdirPushOutcome::Rejected,
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

    /// The account a conflicted store is grouped, and counted, under.
    const CONFLICT_ACCOUNT: &str = "cards";

    /// Seeds one conflicted card with all three bodies present.
    ///
    /// The state [`merge_conflicts`] starts from, so a test reaches it without
    /// a server: what needs a connection is the fetch that lands the remote
    /// body, and that is the other half of [`resolve_conflicts`].
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
        let stored = |body: &str| PimdirWriteOp::StoreObject {
            object: PimdirObject {
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
                PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                    collection: PimdirCollectionId("contacts".into()),
                    handle: PimdirHandle("card1".into()),
                    link_id: Some(PimdirLinkId("uid:a".into())),
                    object: Some(blobs.hash(local.as_bytes())),
                    level: PimdirLevel::Full,
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    flags: PimdirFlags::default(),
                    status: PimdirStatus::Conflict,
                    conflict_revision: Some(String::from("etag-2")),
                    conflict_object: Some(blobs.hash(remote.as_bytes())),
                    base: Some(PimdirBase {
                        flags: PimdirFlags::default(),
                        revision: Some(String::from("etag-1")),
                        object: Some(blobs.hash(base.as_bytes())),
                    }),
                    origin: None,
                }),
            ])
            .unwrap();

        store
    }

    /// A card carrying `tel` and `note`, the fields the merge tests move.
    fn card(tel: &str, note: &str) -> String {
        format!(
            "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:{tel}\r\nNOTE:{note}\r\nEND:VCARD\r\n"
        )
    }

    /// Two sides editing different fields have said nothing contradictory.
    ///
    /// The base names which side touched which, both survive, the conflict
    /// clears through the queue, and the run reports nothing. Asking a person
    /// is a background tool asking to be switched off.
    #[cfg(feature = "dav")]
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
        assert_ne!(placement.status, PimdirStatus::Conflict);
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

        let mut report = SyncOutput::default();
        itemize_pulled(
            &[PimdirSyncEvent::Conflicted(PimdirHandle("card1".into()))],
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

    /// A side the store decides keeps the store's body over its own edit.
    ///
    /// The engine parks a store-wins divergence on a pass that cannot push,
    /// and the pushing pass never revisits a parked one, so the target kept its
    /// edit and the run asked a person what `one-way` had already answered.
    #[test]
    fn a_side_the_store_decides_keeps_the_store_body_against_the_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_with_conflict(
            dir.path(),
            &card("+1", "old"),
            &card("+2", "old"),
            &card("+3", "old"),
        );
        let blobs = store.blobs();

        let kept = keep_store_bodies("contacts", "dav", &mut store, &blobs).unwrap();
        assert_eq!(kept, 1);

        let placement = load_side(&store, "contacts").unwrap().remove(0);
        assert_eq!(placement.status, PimdirStatus::Dirty, "a push is owed");
        assert_eq!(
            placement.object,
            Some(blobs.hash(card("+2", "old").as_bytes()))
        );
        assert!(placement.conflict_revision.is_none());
        assert_eq!(
            placement.base.and_then(|base| base.revision).as_deref(),
            Some("etag-2"),
            "the overwrite is conditioned on the revision the endpoint holds",
        );
    }

    /// A conflict the engine marked reaches the report the account prints.
    ///
    /// The fold is [`SyncOutput::absorb`], the one a worker's collection report
    /// travels back through. Merging only the item patch left the account
    /// report with an empty `conflicts`, so nothing named what it had parked.
    #[test]
    fn a_conflict_the_engine_marked_survives_the_report_merge() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_with_local_edit(dir.path(), "v1");
        let mut remote = MutableRemote::at("card1", "v9");

        let before = flag_snapshot(&store, "contacts", "left").unwrap();
        let sync = sync_with(&mut store, &mut remote, PimdirPushRights::all());
        assert_eq!(sync.conflicts, 1, "the engine parked it");

        let mut collection = SyncOutput::default();
        itemize_pulled(
            &sync.events,
            &before,
            &store,
            "contacts",
            "contacts",
            "left",
            &mut collection,
        )
        .unwrap();
        assert_eq!(collection.conflicts.len(), 1, "the collection names it");

        let mut account = SyncOutput::default();
        account.absorb(collection);
        assert_eq!(
            account.conflicts.len(),
            1,
            "and so does the account report, which is the one printed and the \
             one the notification is raised from",
        );

        let text = account.to_string();
        assert!(text.contains("Warnings (1)"), "{text}");
        assert!(text.contains("card1"), "{text}");
    }

    /// Both sides setting the same field is the residue no merge settles.
    ///
    /// It stays parked and is counted from the store, which is the number the
    /// run's own exit code answers with 2.
    #[cfg(feature = "dav")]
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
        assert_eq!(placement.status, PimdirStatus::Conflict);
        assert_eq!(placement.object, Some(local), "the local side is untouched");

        let mut collection = SyncOutput::default();
        itemize_pulled(
            &[PimdirSyncEvent::Conflicted(PimdirHandle("card1".into()))],
            &HashMap::new(),
            &store,
            "contacts",
            "contacts",
            "dav",
            &mut collection,
        )
        .unwrap();
        assert_eq!(collection.conflicts.len(), 1);

        // NOTE: a worker fills the collection's report, the account's is
        // printed: asserting the first alone is how the arm went missing.
        let mut report = SyncOutput::default();
        report.absorb(collection);
        assert_eq!(report.conflicts.len(), 1);

        count_conflicts(&store, CONFLICT_ACCOUNT, &mut report);
        assert_eq!(report.outstanding_conflicts, 1);
        assert!(report.left_waiting(), "which is what exits with 2");

        let text = report.to_string();
        assert!(text.contains("changed on both sides"), "{text}");
    }

    /// Two fields disagreeing is one item waiting, and two such items are two.
    ///
    /// The merge counts colliding fields and the report counts parked items,
    /// which are not the same number: a summary reading `1 item(s) waiting`
    /// over a two-field divergence would have passed everything else.
    #[cfg(feature = "dav")]
    #[test]
    fn two_colliding_fields_park_the_item_and_the_count_reads_past_one() {
        let dir = tempfile::tempdir().unwrap();
        let base = card("+1", "old");
        let local = card("+2", "mine");
        let remote = card("+3", "theirs");

        assert_eq!(
            Kind::Vcard.merge(base.as_bytes(), local.as_bytes(), remote.as_bytes()),
            Merged::Collided(2),
            "the phone number and the note each disagree, and the merge counts \
             both rather than stopping at the first",
        );

        let mut store = store_with_conflict(dir.path(), &base, &local, &remote);
        let blobs = store.blobs();

        // NOTE: a second item diverging the same way, over the same three
        // bodies: the report prints a number, and one item cannot tell a
        // count from a flag.
        store
            .write(vec![PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId("contacts".into()),
                handle: PimdirHandle("card2".into()),
                link_id: Some(PimdirLinkId("uid:b".into())),
                object: Some(blobs.hash(local.as_bytes())),
                level: PimdirLevel::Full,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Conflict,
                conflict_revision: Some(String::from("etag-2")),
                conflict_object: Some(blobs.hash(remote.as_bytes())),
                base: Some(PimdirBase {
                    flags: PimdirFlags::default(),
                    revision: Some(String::from("etag-1")),
                    object: Some(blobs.hash(base.as_bytes())),
                }),
                origin: None,
            })])
            .unwrap();

        let merged = merge_conflicts(
            "contacts",
            Kind::Vcard,
            "dav",
            &mut store,
            &blobs,
            dir.path(),
        )
        .unwrap();
        assert_eq!(
            merged, 0,
            "a body carrying a field nobody can pick is staged in part or not \
             at all, and it is not at all",
        );

        let parked = load_side(&store, "contacts").unwrap();
        assert_eq!(parked.len(), 2);
        for placement in &parked {
            assert_eq!(placement.status, PimdirStatus::Conflict);
            assert_eq!(
                placement.object,
                Some(blobs.hash(local.as_bytes())),
                "the local side of {} is untouched",
                placement.handle.0,
            );
        }

        let mut collection = SyncOutput::default();
        itemize_pulled(
            &[
                PimdirSyncEvent::Conflicted(PimdirHandle("card1".into())),
                PimdirSyncEvent::Conflicted(PimdirHandle("card2".into())),
            ],
            &HashMap::new(),
            &store,
            "contacts",
            "contacts",
            "dav",
            &mut collection,
        )
        .unwrap();

        let mut report = SyncOutput::default();
        report.absorb(collection);
        assert_eq!(
            report.conflicts.len(),
            2,
            "each item is named once, however many of its fields collided",
        );

        count_conflicts(&store, CONFLICT_ACCOUNT, &mut report);
        assert_eq!(report.outstanding_conflicts, 2);
        assert!(report.left_waiting(), "which is what exits with 2");

        let text = report.to_string();
        assert!(text.contains("Warnings (2)"), "{text}");
        assert!(
            text.contains("Conflicts: 2 item(s) waiting for a decision"),
            "{text}",
        );
    }

    /// A write the remote would not take is not a write the run made.
    ///
    /// The item patch is the plan, derived before anything is pushed, so a
    /// refused `PUT` used to be counted among the hunks the run applied:
    /// `synchronized: 1 hunks`, exit 0, and the same phantom on every run.
    #[test]
    fn a_refused_write_is_reported_instead_of_counted_as_a_hunk() {
        let mut report = SyncOutput::default();
        for id in ["card1", "card2"] {
            report.item.patch.push(PatchEntry::new(
                ItemHunk::Update {
                    side: String::from("dav"),
                    collection: String::from("Default"),
                    id: String::from(id),
                    content_key: 0,
                },
                None,
            ));
        }

        itemize_rejected(
            "dav",
            vec![RejectedPush {
                collection: String::from("Default"),
                handle: String::from("card1"),
                action: "update",
                reason: String::from("HTTP 403: Resource is not a vCard object"),
            }],
            &mut report,
        );

        let ids: Vec<&ItemHunk> = report.item.patch.iter().map(|entry| &entry.hunk).collect();
        assert_eq!(ids.len(), 1, "the refused write left the patch: {ids:?}");
        assert!(format!("{:?}", ids[0]).contains("card2"), "{ids:?}");

        let text = report.to_string();
        assert!(text.contains("Warnings (1)"), "{text}");
        assert!(text.contains("refused the update of card1"), "{text}");
        assert!(text.contains("synchronized: 1 hunks"), "{text}");
        assert_eq!(
            Exit::from(&report),
            Exit::Conflicted,
            "a run that could not deliver a write is not a run that succeeded",
        );
    }

    /// A refused append names the item and takes its copy back.
    ///
    /// The hub stages an append under the item's link id with a marker, the
    /// item having no handle on the side it is created on. Reported raw the
    /// marker reads as part of the name, and the copy hunk was left in.
    #[test]
    fn a_refused_append_is_named_by_its_link_id_and_its_copy_is_taken_back() {
        let mut report = SyncOutput::default();
        for id in ["event-1", "event-2"] {
            report.item.patch.push(PatchEntry::new(
                ItemHunk::Copy {
                    source_side: String::from("local"),
                    target_side: String::from("dav"),
                    collection: String::from("Calendar"),
                    source_id: String::from(id),
                    flags: BTreeSet::new(),
                    origin: None,
                    content_key: 0,
                },
                None,
            ));
        }

        itemize_rejected(
            "dav",
            vec![RejectedPush {
                collection: String::from("Calendar"),
                handle: String::from("event-1\u{1}hub"),
                action: "append",
                reason: String::from("HTTP 403"),
            }],
            &mut report,
        );

        let left: Vec<&ItemHunk> = report.item.patch.iter().map(|entry| &entry.hunk).collect();
        assert_eq!(left.len(), 1, "the refused append left the patch: {left:?}");
        assert!(format!("{:?}", left[0]).contains("event-2"), "{left:?}");

        let text = report.to_string();
        assert!(text.contains("refused the append of event-1 in"), "{text}");
        assert!(!text.contains("event-1\u{1}"), "the marker is printed raw");
    }

    /// A conflict an earlier run parked reaches no later run's report.
    ///
    /// The engine returns early for a placement it already marked, so there is
    /// nothing to announce. The store still holds the decision, which is why
    /// the two numbers are read from two places.
    #[test]
    fn a_second_run_over_an_unresolved_conflict_announces_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with_conflict(
            dir.path(),
            &card("+1", "old"),
            &card("+2", "old"),
            &card("+3", "old"),
        );

        let mut report = SyncOutput::default();
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

    /// A create a frontend staged must still read back as a create.
    ///
    /// The engine pushes an add only for a `Created` placement, so a status the
    /// store cannot hold back never reaches the server. A two-replica account
    /// looped against Fastmail so: six staged cards, six hunks, none received.
    #[test]
    fn a_queued_create_reads_back_as_a_create() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path())
            .unwrap()
            .for_account(CONFLICT_ACCOUNT)
            .for_source("dav");
        store
            .ensure_collection("dav/contacts", "text/vcard")
            .unwrap();

        let body = card("+1", "new");
        let blobs = store.blobs();
        let hash = blobs.hash(body.as_bytes());
        let mut writer = blobs.writer().unwrap();
        writer.write_all(body.as_bytes()).unwrap();
        let object = PimdirObject {
            hash: hash.clone(),
            size: writer.commit(&hash).unwrap() as usize,
        };

        let mut producer = PimdirProducer::open(dir.path(), "test-frontend").unwrap();
        producer
            .enqueue(
                "dav/contacts",
                &PimdirAction::Add {
                    link_id: Some(PimdirLinkId("uid:new".into())),
                    flags: PimdirFlags::default(),
                    object: Some(hash),
                },
                Some(&object),
            )
            .unwrap();
        drop(producer);

        assert_eq!(store.drain().unwrap().applied, 1);

        let placement = load_side(&store, "dav/contacts").unwrap().remove(0);
        assert_eq!(
            placement.status,
            PimdirStatus::Created,
            "a staged create the store reads back as {:?} is never pushed",
            placement.status,
        );
    }

    /// Seeds a store holding one locally edited item.
    ///
    /// Its body points at `edited`, its base at `original` and
    /// `base_revision`: what a frontend leaves after staging an edit offline.
    fn store_with_local_edit(dir: &std::path::Path, base_revision: &str) -> PimdirSourceStore {
        let mut store = PimdirStore::open(dir).unwrap().for_source("left");
        store.ensure_collection("contacts", "text/vcard").unwrap();
        store
            .write(vec![
                PimdirWriteOp::StoreObject {
                    object: PimdirObject {
                        hash: PimdirHash("0rig".into()),
                        size: 3,
                    },
                    body: Some(b"old".to_vec()),
                },
                PimdirWriteOp::StoreObject {
                    object: PimdirObject {
                        hash: PimdirHash("ed17".into()),
                        size: 3,
                    },
                    body: Some(b"new".to_vec()),
                },
                PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                    collection: PimdirCollectionId("contacts".into()),
                    handle: PimdirHandle("card1".into()),
                    link_id: Some(PimdirLinkId("uid:a".into())),
                    object: Some(PimdirHash("ed17".into())),
                    level: PimdirLevel::Full,
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    flags: PimdirFlags::default(),
                    status: PimdirStatus::Dirty,
                    conflict_revision: None,
                    conflict_object: None,
                    base: Some(PimdirBase {
                        flags: PimdirFlags::default(),
                        revision: Some(base_revision.to_string()),
                        object: Some(PimdirHash("0rig".into())),
                    }),
                    origin: None,
                }),
            ])
            .unwrap();
        store
    }

    /// One side of a two-endpoint account, holding one locally edited card.
    ///
    /// Both sides hold the same item under the same link id, which is what an
    /// account mirroring two servers is: one hub item, one binding per
    /// endpoint, each with a base of its own.
    fn endpoint_with_local_edit(
        dir: &std::path::Path,
        source: &str,
        base_revision: &str,
    ) -> PimdirSourceStore {
        let mut store = PimdirStore::open(dir).unwrap().for_source(source);
        store.ensure_collection("contacts", "text/vcard").unwrap();
        store
            .write(vec![
                PimdirWriteOp::StoreObject {
                    object: PimdirObject {
                        hash: PimdirHash("0rig".into()),
                        size: 3,
                    },
                    body: Some(b"old".to_vec()),
                },
                PimdirWriteOp::StoreObject {
                    object: PimdirObject {
                        hash: PimdirHash("ed17".into()),
                        size: 3,
                    },
                    body: Some(b"new".to_vec()),
                },
                PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                    collection: PimdirCollectionId("contacts".into()),
                    handle: PimdirHandle("card1".into()),
                    link_id: Some(PimdirLinkId("uid:a".into())),
                    object: Some(PimdirHash("ed17".into())),
                    level: PimdirLevel::Full,
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    flags: PimdirFlags::default(),
                    status: PimdirStatus::Dirty,
                    conflict_revision: None,
                    conflict_object: None,
                    base: Some(PimdirBase {
                        flags: PimdirFlags::default(),
                        revision: Some(base_revision.to_string()),
                        object: Some(PimdirHash("0rig".into())),
                    }),
                    origin: None,
                }),
            ])
            .unwrap();
        store
    }

    /// Puts a parked divergence back the way this run's own merge leaves it.
    ///
    /// Dirty again, rebased on the revision it conflicted at, so the next pass
    /// pushes and can meet a remote that moved once more.
    fn settle_as_the_merge_would(store: &mut PimdirSourceStore) {
        let mut placement = load_side(store, "contacts").unwrap().remove(0);
        let revision = placement.conflict_revision.take();

        placement.status = PimdirStatus::Dirty;
        placement.conflict_object = None;
        placement.base = Some(PimdirBase {
            flags: PimdirFlags::default(),
            revision,
            object: Some(PimdirHash("0rig".into())),
        });

        store
            .write(vec![PimdirWriteOp::UpsertPlacement(placement)])
            .unwrap();
    }

    /// A source and a target both park a divergence, each named once.
    ///
    /// `reconcile_pass` asked whether anything moved and threw the events away,
    /// so a two-endpoint account reported no conflict anywhere. Naive itemizing
    /// double-counts instead, which is why this drives more than one pass.
    #[test]
    fn a_conflict_on_each_endpoint_is_named_once_across_a_convergence_loop() {
        let dir = tempfile::tempdir().unwrap();
        let mut left = endpoint_with_local_edit(dir.path(), "left", "l1");
        let mut right = endpoint_with_local_edit(dir.path(), "right", "r1");
        let mut left_remote = MutableRemote::at("card1", "l9");
        let mut right_remote = MutableRemote::at("card1", "r9");

        let mut report = SyncOutput::default();
        for pass in 0..2 {
            let left_pass = sync_with(&mut left, &mut left_remote, PimdirPushRights::all());
            itemize_conflicted(
                &left_pass.events,
                &left,
                "contacts",
                "contacts",
                "left",
                &mut report,
            )
            .unwrap();

            let right_pass = sync_with(&mut right, &mut right_remote, PimdirPushRights::all());
            itemize_conflicted(
                &right_pass.events,
                &right,
                "contacts",
                "contacts",
                "right",
                &mut report,
            )
            .unwrap();

            assert_eq!(
                (pass, left_pass.conflicts),
                (pass, 1),
                "the engine parked the left side on pass {pass}",
            );

            // NOTE: what the run's own merge does to a divergence it settles,
            // so the next pass genuinely marks this one again.
            settle_as_the_merge_would(&mut left);
            left_remote
                .items
                .insert(String::from("card1"), (String::from("l10"), None));
        }

        assert_eq!(
            report.conflicts.len(),
            2,
            "one line per endpoint, not one per pass: {:?}",
            report.conflicts,
        );
        let sides: Vec<&str> = report
            .conflicts
            .iter()
            .map(|conflict| conflict.side.as_str())
            .collect();
        assert_eq!(sides, ["left", "right"]);

        assert_eq!(
            warn_conflicts(&report),
            2,
            "and one announcement per item entering conflict",
        );

        // NOTE: a later run: both are parked already, so the engine says
        // nothing.
        let mut later = SyncOutput::default();
        for (store, remote, side) in [
            (&mut left, &mut left_remote, "left"),
            (&mut right, &mut right_remote, "right"),
        ] {
            let pass = sync_with(store, remote, PimdirPushRights::all());
            itemize_conflicted(
                &pass.events,
                store,
                "contacts",
                "contacts",
                side,
                &mut later,
            )
            .unwrap();
        }

        assert!(later.conflicts.is_empty());
        assert_eq!(
            warn_conflicts(&later),
            0,
            "a conflict an earlier run parked is announced by no later one",
        );
    }

    /// One side of a two-endpoint account, holding one reconciled card.
    ///
    /// Clean against its own server at `revision`, so what a pull round finds
    /// is the server's own doing and nothing this run staged.
    fn endpoint_holding(dir: &std::path::Path, source: &str, revision: &str) -> PimdirSourceStore {
        let mut store = PimdirStore::open(dir).unwrap().for_source(source);
        store.ensure_collection("contacts", "text/vcard").unwrap();
        store
            .write(vec![
                PimdirWriteOp::StoreObject {
                    object: PimdirObject {
                        hash: PimdirHash("0rig".into()),
                        size: 3,
                    },
                    body: Some(b"old".to_vec()),
                },
                PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                    collection: PimdirCollectionId("contacts".into()),
                    handle: PimdirHandle("card1".into()),
                    link_id: Some(PimdirLinkId("uid:a".into())),
                    object: Some(PimdirHash("0rig".into())),
                    level: PimdirLevel::Full,
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    flags: PimdirFlags::default(),
                    status: PimdirStatus::Clean,
                    conflict_revision: None,
                    conflict_object: None,
                    base: Some(PimdirBase {
                        flags: PimdirFlags::default(),
                        revision: Some(revision.to_string()),
                        object: Some(PimdirHash("0rig".into())),
                    }),
                    origin: None,
                }),
            ])
            .unwrap();
        store
    }

    /// A server's own delete is named once on each side.
    ///
    /// The two-endpoint path read only conflicts out of its pass, so a card a
    /// server dropped reached the report as the propagation alone. Itemizing
    /// a pushing pass instead double-counts, so the opening round only pulls.
    #[test]
    fn a_server_delete_is_named_once_on_each_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let mut left = endpoint_holding(dir.path(), "left", "l1");
        let right = endpoint_holding(dir.path(), "right", "r1");

        let mut left_remote = MutableRemote::at("card1", "l1");
        left_remote.items.clear();

        let before = flag_snapshot(&left, "contacts", "left").unwrap();
        let pull = run_verb(
            &mut left,
            &mut left_remote,
            sync_verb(
                "contacts",
                false,
                PimdirPushRights::all(),
                PimdirConflictPolicy::Manual,
                PimdirScope::unbounded(),
                true,
            ),
        )
        .unwrap();
        assert!(
            pull.events
                .iter()
                .any(|event| matches!(event, PimdirSyncEvent::Vanished(_))),
            "the engine saw the card go: {:?}",
            pull.events,
        );

        let mut report = SyncOutput::default();
        itemize_pulled(
            &pull.events,
            &before,
            &left,
            "contacts",
            "contacts",
            "left",
            &mut report,
        )
        .unwrap();

        for (source, other) in [("left", "right"), ("right", "left")] {
            for placement in projection_view(&right, "contacts", source).unwrap() {
                for hunk in placement_hunks(source, other, source, "contacts", &placement) {
                    report.item.patch.push(PatchEntry::new(hunk, None));
                }
            }
        }

        let deletes: Vec<&str> = report
            .item
            .patch
            .iter()
            .filter_map(|entry| match &entry.hunk {
                ItemHunk::Delete { side, .. } => Some(side.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            deletes,
            ["left", "right"],
            "the pull names it on left, the projection owes it on right: {:?}",
            report.item.patch,
        );
    }

    /// A store holding the tombstone a frontend leaves after an offline delete.
    fn store_with_local_delete(dir: &std::path::Path) -> PimdirSourceStore {
        let mut store = PimdirStore::open(dir).unwrap().for_source("left");
        store.ensure_collection("contacts", "text/vcard").unwrap();
        store
            .write(vec![
                PimdirWriteOp::StoreObject {
                    object: PimdirObject {
                        hash: PimdirHash("0rig".into()),
                        size: 3,
                    },
                    body: Some(b"old".to_vec()),
                },
                PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                    collection: PimdirCollectionId("contacts".into()),
                    handle: PimdirHandle("card1".into()),
                    link_id: Some(PimdirLinkId("uid:a".into())),
                    object: Some(PimdirHash("0rig".into())),
                    level: PimdirLevel::Full,
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    flags: PimdirFlags::default(),
                    status: PimdirStatus::Tombstone,
                    conflict_revision: None,
                    conflict_object: None,
                    base: Some(PimdirBase {
                        flags: PimdirFlags::default(),
                        revision: Some(String::from("v1")),
                        object: Some(PimdirHash("0rig".into())),
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
        rights: PimdirPushRights,
    ) -> PimdirSyncReport {
        run_verb(
            store,
            remote,
            sync_verb(
                "contacts",
                true,
                rights,
                PimdirConflictPolicy::Manual,
                PimdirScope::unbounded(),
                true,
            ),
        )
        .unwrap()
    }

    /// A side that may not delete holds the tombstone instead of undoing it.
    ///
    /// Both refusals run through one disposition, and for a hub-backed store it
    /// has to be `Keep`: undoing the delete says the source still holds the
    /// member, so the hub clears the deletion and the item returns everywhere.
    #[test]
    fn a_side_that_may_not_delete_keeps_the_tombstone() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_with_local_delete(dir.path());
        let mut remote = MutableRemote::at("card1", "v1");

        let rights = PimdirPushRights {
            remove: false,
            ..PimdirPushRights::all()
        };
        let report = sync_with(&mut store, &mut remote, rights);

        assert_eq!(report.pushed, 0, "the delete is not pushed");
        assert!(remote.pushed.is_empty(), "and nothing else is either");

        let placements = store
            .load(
                &PimdirCollectionId("contacts".into()),
                &PimdirLoadScope::All,
            )
            .unwrap()
            .placements;
        assert_eq!(placements.len(), 1);
        assert_eq!(
            placements[0].status,
            PimdirStatus::Tombstone,
            "the removal is still a removal, not undone into a clean row",
        );
    }

    /// A flag on a card or a calendar item is the store's alone (a `pimdir
    /// tag`): it is never pushed, so the backend rejects nothing and warns
    /// nothing on every run, and the flag stays. Mail keeps the right.
    #[test]
    fn a_store_only_flag_on_a_card_stays_local() {
        let perms = SourcePermissions {
            collection: Default::default(),
            flag: Default::default(),
            item: Default::default(),
        };
        assert!(push_rights(&perms, "message/rfc822").flags);
        assert!(!push_rights(&perms, "text/calendar").flags);

        let dir = tempfile::tempdir().unwrap();
        let mut store = store_with_local_delete(dir.path());
        let contacts = PimdirCollectionId("contacts".into());
        let mut placement = store
            .load(&contacts, &PimdirLoadScope::All)
            .unwrap()
            .placements
            .remove(0);
        placement.status = PimdirStatus::Dirty;
        placement.flags = PimdirFlags::from_iter(["$pinned"]);
        store
            .write(vec![PimdirWriteOp::UpsertPlacement(placement)])
            .unwrap();
        let mut remote = MutableRemote::at("card1", "v1");

        for _ in 0..2 {
            sync_with(&mut store, &mut remote, push_rights(&perms, "text/vcard"));
        }

        assert!(remote.pushed.is_empty(), "no flag change pushed");
        let placements = store
            .load(&contacts, &PimdirLoadScope::All)
            .unwrap()
            .placements;
        assert_eq!(
            placements[0].flags,
            PimdirFlags::from_iter(["$pinned"]),
            "the flag kept in the store"
        );
    }

    #[test]
    fn a_local_body_edit_is_pushed_conditionally_and_confirmed() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_with_local_edit(dir.path(), "v1");
        let mut remote = MutableRemote::at("card1", "v1");

        let report = sync_with(&mut store, &mut remote, PimdirPushRights::all());

        assert!(
            matches!(
                remote.pushed.as_slice(),
                [PimdirChange { kind: PimdirChangeKind::Update { if_match, .. }, .. }]
                    if if_match.as_deref() == Some("v1")
            ),
            "expected one If-Match update, got {:?}",
            remote.pushed
        );
        assert_eq!(report.conflicts, 0);
        assert_eq!(report.rejected, 0);

        assert_eq!(
            remote.items["card1"],
            (String::from("v2"), Some(PimdirHash("ed17".into())))
        );
        let placements = load_side(&store, "contacts").unwrap();
        assert_eq!(placements[0].status, PimdirStatus::Clean);
        let base = placements[0].base.as_ref().expect("a rebased base");
        assert_eq!(base.revision.as_deref(), Some("v2"));
        assert_eq!(base.object, Some(PimdirHash("ed17".into())));
    }

    #[test]
    fn a_body_edited_on_both_sides_is_left_conflicted_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_with_local_edit(dir.path(), "v1");
        let mut remote = MutableRemote::at("card1", "v9");

        let report = sync_with(&mut store, &mut remote, PimdirPushRights::all());

        assert_eq!(
            remote.items["card1"].1, None,
            "remote must not be clobbered"
        );
        assert_eq!(report.conflicts, 1);
        assert!(
            report
                .events
                .iter()
                .any(|e| matches!(e, PimdirSyncEvent::Conflicted(h) if h.0 == "card1")),
            "expected a Conflicted event, got {:?}",
            report.events
        );

        let placements = load_side(&store, "contacts").unwrap();
        assert_eq!(
            placements[0].status,
            PimdirStatus::Conflict,
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

        let rights = PimdirPushRights {
            content: false,
            ..PimdirPushRights::all()
        };
        sync_with(&mut store, &mut remote, rights);

        assert!(
            remote.pushed.is_empty(),
            "a forbidden update must not reach the remote, got {:?}",
            remote.pushed
        );
        let placements = load_side(&store, "contacts").unwrap();
        assert_eq!(placements[0].status, PimdirStatus::Dirty);
        assert_eq!(placements[0].object, Some(PimdirHash("ed17".into())));
    }

    /// A `Meta`-level placement with a base, as after a first reconcile,
    /// summarised with the octet length a relay needs.
    fn linked(handle: &str, link: &str, size: Option<u64>) -> PimdirPlacement {
        PimdirPlacement {
            collection: PimdirCollectionId("INBOX".into()),
            handle: PimdirHandle(handle.into()),
            link_id: Some(PimdirLinkId(link.into())),
            object: None,
            level: PimdirLevel::Meta,
            summary: Some(PimdirSummary::Mail(PimdirMailSummary {
                size,
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
        }
    }

    /// Two copies of one identity are both ordinary items.
    ///
    /// Where the freeze this reverses left one of them out of the store and
    /// warned, the second now carries a minted key, projects a copy like any
    /// other item, and the run says nothing of its own about the pair.
    #[test]
    fn a_duplicated_identity_is_two_items_and_no_warning() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
        store.ensure_collection("INBOX", "message/rfc822").unwrap();

        store
            .write(vec![
                PimdirWriteOp::UpsertPlacement(linked("145", "a@x", None)),
                PimdirWriteOp::UpsertPlacement(linked("146", "dup:a@x#146", None)),
            ])
            .unwrap();

        let view = projection_view(&store, "INBOX", "left").unwrap();
        assert_eq!(view.len(), 2, "both copies are stored and projected");

        let mut report = SyncOutput {
            account: "dup".into(),
            ..Default::default()
        };
        for placement in &view {
            for hunk in placement_hunks("left", "right", "left", "INBOX", placement) {
                report.item.patch.push(PatchEntry::new(hunk, None));
            }
        }

        let text = report.to_string();
        assert!(!text.contains("Warnings"), "{text}");

        let json = serde_json::to_value(&report).unwrap();
        assert!(json.get("ambiguous").is_none(), "{json}");
        assert!(json.get("refused").is_none(), "{json}");
    }

    /// A target refusing the second copy is the one thing worth a line.
    ///
    /// The run wrote nothing for that item, and the line carries the `UID` and
    /// the collection the user repairs it by.
    #[test]
    fn a_refused_duplicate_is_named_with_its_uid() {
        let refused = vec![RefusedCreate {
            collection: String::from("agenda"),
            uid: String::from("event-1@google.com"),
            handle: String::from("copy-1.ics"),
        }];

        let mut report = SyncOutput {
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

    /// A refusal is reported once however many passes met it.
    ///
    /// A collection reconciles until quiescent, so the create is re-derived and
    /// answered the same way each pass. Two copies of one identity are still
    /// two refusals, which is the distinction the handle keeps.
    #[test]
    fn a_refusal_re_observed_on_a_later_pass_is_still_one_refusal() {
        let refused = |handle: &str| RefusedCreate {
            collection: String::from("agenda"),
            uid: String::from("event-1@google.com"),
            handle: String::from(handle),
        };

        let mut report = SyncOutput::default();
        itemize_refused(
            "right",
            vec![
                refused("copy-1.ics"),
                refused("copy-1.ics"),
                refused("copy-2.ics"),
            ],
            &mut report,
        );

        assert_eq!(
            report.refused.len(),
            2,
            "one line per copy, however many passes met the same answer",
        );
    }

    /// The same for a rejected write: re-derived every pass, reported once.
    #[test]
    fn a_write_refused_on_every_pass_is_still_one_line() {
        let rejected = |handle: &str| RejectedPush {
            collection: String::from("Default"),
            handle: String::from(handle),
            action: "update",
            reason: String::from("HTTP 403: Resource is not a vCard object"),
        };

        let mut report = SyncOutput::default();
        report.item.patch.push(PatchEntry::new(
            ItemHunk::Update {
                side: String::from("dav"),
                collection: String::from("Default"),
                id: String::from("card1"),
                content_key: 0,
            },
            None,
        ));
        itemize_rejected(
            "dav",
            vec![rejected("card1"), rejected("card1"), rejected("card2")],
            &mut report,
        );

        assert_eq!(report.rejected.len(), 2);
        assert!(report.item.patch.is_empty(), "and the hunk is still gone");
    }

    #[test]
    fn a_relayed_copy_is_itemized_so_the_run_never_reads_as_already_in_sync() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("left");
        store.ensure_collection("INBOX", "message/rfc822").unwrap();

        store
            .write(vec![PimdirWriteOp::UpsertPlacement(linked(
                "1",
                "mid:a@x",
                Some(42),
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
        let mut report = SyncOutput {
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
                origin: None,
                content_key: content_key(&target.link),
            },
            None,
        ));

        let text = report.to_string();
        assert!(!text.contains("already in sync"), "{text}");
        assert!(text.contains("synchronized: 1 hunks"), "{text}");
        assert!(text.contains("from left to right"), "{text}");
    }

    /// What keeps a relay from repeating itself.
    ///
    /// A relay bypasses the engine, so nothing else writes the copy down: an
    /// item is selected for being on one source alone, and only the target's
    /// binding takes it out. Without it every pass relays the same body again.
    #[test]
    fn a_relayed_copy_is_bound_to_its_target_and_never_relayed_twice() {
        let dir = tempfile::tempdir().unwrap();
        let mut left = PimdirStore::open(dir.path()).unwrap().for_source("left");
        left.ensure_collection("INBOX", "message/rfc822").unwrap();

        left.write(vec![PimdirWriteOp::UpsertPlacement(linked(
            "1",
            "mid:a@x",
            Some(42),
        ))])
        .unwrap();

        let sides = |store: &PimdirStore| {
            relay_targets(store, Kind::Mail, "INBOX", ("left", false), ("right", true)).unwrap()
        };
        let targets = sides(&left);
        assert_eq!(targets.len(), 1);

        let mut right = PimdirStore::open(dir.path()).unwrap().for_source("right");
        let written = WrittenItem {
            id: String::from("77"),
            revision: None,
        };
        bind_relayed(&mut right, "INBOX", &targets[0], written).unwrap();

        assert!(sides(&right).is_empty(), "the copy is not relayed again");

        let hub = right.load_hub("INBOX").unwrap();
        let item = hub.items.get(&PimdirLinkId::from("mid:a@x")).unwrap();
        assert_eq!(item.sources.len(), 2, "the item is two-sided now");

        let binding = &item.sources[&source_id("right")];
        assert_eq!(binding.handle, PimdirHandle::from("77"), "as assigned");
        assert!(
            binding.base.is_some(),
            "and based, so the next sync reads it as in sync rather than as a create",
        );
        assert!(item.object.is_none(), "the relay stored no body");
        assert_eq!(item.level, PimdirLevel::Meta, "so it claims no more");
    }

    /// A hub collection id carries its namespace and the wire name does not.
    ///
    /// Getting this backwards is how a mailbox and an address book both called
    /// `Default` would end up as one collection.
    #[test]
    fn a_hub_id_carries_its_namespace_and_the_display_name_does_not() {
        assert_eq!(hub_id("mail", "INBOX"), "mail/INBOX");
        assert_eq!(display_name("mail", "mail/INBOX"), "INBOX");

        assert_eq!(hub_id("mail", "Archive/2026"), "mail/Archive/2026");
        assert_eq!(display_name("mail", "mail/Archive/2026"), "Archive/2026");
        assert_eq!(display_name("mail", "mailbox/INBOX"), "mailbox/INBOX");
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

    /// Stages one `submit` in the store at `dir`, anchored on `collection`.
    fn queue_submit(dir: &Path, collection: &str) {
        PimdirProducer::open(dir, "test-frontend")
            .unwrap()
            .enqueue(
                collection,
                &PimdirAction::Unknown {
                    kind: submit::SUBMIT.into(),
                    payload: r#"{"v":1,"from":"a@x.org","rcpts":["b@y.org"]}"#.into(),
                    object_hash: None,
                },
                None,
            )
            .unwrap();
    }

    /// A run performing intents alone opens an endpoint for a send only
    /// when the endpoint sends it natively: an `smtp` table opens alone.
    #[test]
    fn a_send_needs_its_source_opened_only_when_it_sends_natively() {
        let smtp: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            smtp.server = "smtps://smtp.example.org:465"
            "#,
        )
        .unwrap();
        let graph: AccountConfig = toml::from_str(
            r#"
            msgraph.auth.token.raw = "tok"
            "#,
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let store = PimdirStore::open(dir.path()).unwrap().for_source("imap");
        store
            .ensure_collection("imap/Sent", "message/rfc822")
            .unwrap();
        queue_submit(dir.path(), "imap/Sent");
        assert!(wanted_sides(&store, &smtp, &["imap"], "imap").is_empty());

        let dir = tempfile::tempdir().unwrap();
        let store = PimdirStore::open(dir.path()).unwrap().for_source("msgraph");
        store
            .ensure_collection("msgraph/Sent Items", "message/rfc822")
            .unwrap();
        assert!(wanted_sides(&store, &graph, &["msgraph"], "msgraph").is_empty());
        queue_submit(dir.path(), "msgraph/Sent Items");
        assert_eq!(
            wanted_sides(&store, &graph, &["msgraph"], "msgraph"),
            BTreeSet::from([String::from("msgraph")])
        );
    }

    /// The authority is the whole of `one-way`.
    ///
    /// It is what stops a divergence becoming a conflict nobody can resolve.
    #[test]
    fn the_authority_decides_the_conflict_and_the_push() {
        assert_eq!(
            Authority::Shared.conflict_policy(),
            PimdirConflictPolicy::Manual,
        );
        assert!(
            Authority::Shared.writes_back(),
            "both sides write in a two-way mirror",
        );

        assert_eq!(
            Authority::Endpoint.conflict_policy(),
            PimdirConflictPolicy::PreferRemote,
        );
        assert!(
            !Authority::Endpoint.writes_back(),
            "what the source holds is the truth, so nothing is written back to it",
        );

        assert_eq!(
            Authority::Store.conflict_policy(),
            PimdirConflictPolicy::PreferLocal,
        );
        assert!(
            Authority::Store.writes_back(),
            "the target takes what the store pushes",
        );
    }

    /// A kind named by its body arrives with it, its page fetching the bodies.
    ///
    /// The plan read off the store alone would call it done, so a first sync of
    /// a whole address book reported nothing at all: the pull's events name it.
    #[test]
    fn the_pull_plan_names_an_item_its_listing_brought_with_its_body() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("dav");
        store
            .ensure_collection("dav/contacts", "text/vcard")
            .unwrap();

        let object = PimdirHash("beef0001".into());
        store
            .write(vec![
                PimdirWriteOp::StoreObject {
                    object: PimdirObject {
                        hash: object.clone(),
                        size: 3,
                    },
                    body: Some(b"abc".to_vec()),
                },
                PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                    collection: PimdirCollectionId("dav/contacts".into()),
                    handle: PimdirHandle("card-1.vcf".into()),
                    link_id: Some(PimdirLinkId("uid:card-1".into())),
                    object: Some(object.clone()),
                    level: PimdirLevel::Full,
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    flags: PimdirFlags::default(),
                    status: PimdirStatus::Clean,
                    conflict_revision: None,
                    conflict_object: None,
                    base: Some(PimdirBase {
                        flags: PimdirFlags::default(),
                        revision: Some(String::from("etag-1")),
                        object: Some(object),
                    }),
                    origin: None,
                }),
            ])
            .unwrap();

        let mut report = SyncOutput::default();
        itemize_fetches(
            "dav/contacts",
            "contacts",
            &store,
            "dav",
            &[],
            &HashSet::new(),
            &mut report,
        )
        .unwrap();
        assert!(
            report.item.patch.is_empty(),
            "a body held from before is no fetch"
        );

        let added = [PimdirSyncEvent::Added(PimdirHandle("card-1.vcf".into()))];
        itemize_fetches(
            "dav/contacts",
            "contacts",
            &store,
            "dav",
            &added,
            &HashSet::new(),
            &mut report,
        )
        .unwrap();
        assert_eq!(report.item.patch.len(), 1);
        assert!(
            matches!(&report.item.patch[0].hunk, ItemHunk::Fetch { id, .. } if id == "uid:card-1"),
            "named by the identity its body states",
        );
    }

    /// A content change drops the stale object and leaves the level alone.
    ///
    /// A plan keyed on the level therefore calls an item about to be re-fetched
    /// done.
    #[test]
    fn the_pull_plan_names_a_body_dropped_by_a_content_change() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path()).unwrap().for_source("dav");
        store
            .ensure_collection("dav/contacts", "text/vcard")
            .unwrap();

        store
            .write(vec![PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId("dav/contacts".into()),
                handle: PimdirHandle("card-1.vcf".into()),
                link_id: Some(PimdirLinkId("uid:card-1".into())),
                object: None,
                level: PimdirLevel::Full,
                summary: None,
                sort_key: PimdirSortKey::default(),
                flags: PimdirFlags::default(),
                status: PimdirStatus::Clean,
                conflict_revision: None,
                conflict_object: None,
                base: None,
                origin: None,
            })])
            .unwrap();

        let mut report = SyncOutput::default();
        itemize_fetches(
            "dav/contacts",
            "contacts",
            &store,
            "dav",
            &[],
            &HashSet::new(),
            &mut report,
        )
        .unwrap();

        assert_eq!(
            report.item.patch.len(),
            1,
            "Full with no object is a body to re-fetch, not a finished item",
        );
    }

    /// A calendar whose server implements no `sync-collection`.
    ///
    /// Every enumeration is the whole member set, and an identity resolves only
    /// by downloading the body. The reported Posteo shape, one `UID` under two
    /// hrefs.
    struct FullListingRemote {
        /// `handle -> (uid, body)`, in the order the listing returns them.
        items: Vec<(String, String, Vec<u8>)>,
        /// Every handle a body was fetched for, so a re-download is visible.
        fetched: Vec<String>,
        /// What the store binds, `handle -> link id`, which names a listed
        /// member at the same revision without its body, as `PimRemote` does.
        bound: HashMap<String, String>,
    }

    impl PimdirRemote for FullListingRemote {
        type Error = anyhow::Error;

        fn enumerate(
            &mut self,
            _collection: &PimdirCollectionId,
            _request: PimdirEnumerate,
        ) -> Result<PimdirEnumerated, Self::Error> {
            let mut items = Vec::new();
            for (handle, uid, body) in self.items.clone() {
                let meta = match self.bound.get(&handle) {
                    Some(link) => named(link),
                    None => {
                        self.fetched.push(handle.clone());
                        named(&uid).with_body(PimdirFetchedBody::Inline {
                            hash: PimdirHash(format!("{:016x}", digest(&body))),
                            bytes: body,
                        })
                    }
                };
                items.push(PimdirRemoteItem {
                    handle: PimdirHandle(handle),
                    flags: PimdirFlags::from_iter([] as [String; 0]),
                    revision: Some(String::from("etag-1")),
                    meta,
                });
            }
            Ok(PimdirEnumerated::Page(PimdirRemoteSnapshot::round(
                items,
                Some(PimdirCheckpoint(Vec::new())),
            )))
        }

        fn fetch(
            &mut self,
            _collection: &PimdirCollectionId,
            handles: Vec<PimdirHandle>,
            _tier: PimdirTier,
        ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
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
                items.push(PimdirFetchedItem {
                    handle,
                    link_id: PimdirLinkId(uid),
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    body: Some(PimdirFetchedBody::Inline {
                        hash: PimdirHash(format!("{:016x}", digest(&body))),
                        bytes: body,
                    }),
                    revision: Some(String::from("etag-1")),
                });
            }
            Ok(items)
        }

        fn push(
            &mut self,
            _collection: &PimdirCollectionId,
            _changes: Vec<PimdirChange>,
        ) -> Result<Vec<PimdirPushResult>, Self::Error> {
            anyhow::bail!("the listing remote is never pushed to")
        }
    }

    /// A content hash for a fake body, standing in for the store's own hasher.
    ///
    /// Distinct bodies must name distinct objects, or a test proves
    /// deduplication rather than storage.
    fn digest(body: &[u8]) -> u64 {
        body.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
    }

    /// One collection's spine as `collection_spine` runs it for a lone source.
    ///
    /// Pull, the listing bringing the bodies of what the store does not bind,
    /// which is where a DAV identity resolves, then report what came.
    fn spine(
        store: &mut PimdirSourceStore,
        remote: &mut FullListingRemote,
        collection: &str,
    ) -> SyncOutput {
        remote.bound = load_side(store, collection)
            .unwrap()
            .into_iter()
            .filter(|p| p.base.is_some())
            .filter_map(|p| Some((p.handle.0, p.link_id?.0)))
            .collect();

        let mut report = SyncOutput::default();
        let pull = run_verb(
            store,
            remote,
            sync_verb(
                collection,
                false,
                PimdirPushRights::all(),
                PimdirConflictPolicy::Manual,
                PimdirScope::unbounded(),
                true,
            ),
        )
        .unwrap();
        itemize_fetches(
            collection,
            "agenda",
            store,
            "caldav",
            &pull.events,
            &HashSet::new(),
            &mut report,
        )
        .unwrap();
        report
    }

    /// One `UID` under two hrefs mirrors as two items; the next run is silent.
    ///
    /// That second run is the bug the user saw: the frozen twin never got a
    /// row, so the pull plan read it as an unfetched body and named it every
    /// run. The collection is listed in full here, as that server lists it.
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
            bound: HashMap::new(),
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

        // NOTE: the run the user complained about: the same full listing,
        // nothing left to say.
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

    /// What a create delivers by names the hunk: a frontend's compose is an
    /// add, a staged copy is a copy from its origin, and a body the other
    /// side holds is a copy across.
    #[test]
    fn a_create_is_named_by_what_delivers_it() {
        let created = |origin: Option<PimdirOrigin>| PimdirPlacement {
            collection: PimdirCollectionId("imap/INBOX".into()),
            handle: PimdirHandle("\u{1}mid:q@x".into()),
            link_id: Some(PimdirLinkId("mid:q@x".into())),
            object: None,
            level: PimdirLevel::Full,
            summary: None,
            sort_key: PimdirSortKey::default(),
            flags: PimdirFlags::default(),
            status: PimdirStatus::Created,
            conflict_revision: None,
            conflict_object: None,
            base: None,
            origin,
        };

        let add = placement_hunks("imap", "imap", "imap", "INBOX", &created(None));
        assert_eq!(add[0].to_string(), "add item mid:q@x in INBOX on imap");

        let origin = PimdirOrigin {
            collection: PimdirCollectionId("imap/Sent".into()),
            handle: PimdirHandle("7".into()),
        };
        let copy = placement_hunks("imap", "imap", "imap", "INBOX", &created(Some(origin)));
        assert_eq!(
            copy[0].to_string(),
            "copy item mid:q@x from Sent to INBOX on imap"
        );

        let across = placement_hunks("left", "right", "left", "INBOX", &created(None));
        assert_eq!(
            across[0].to_string(),
            "copy item mid:q@x in INBOX from right to left"
        );
    }

    /// A fallback that must never be reached: every body comes from its batch.
    struct NoWire;

    impl PimdirRemote for NoWire {
        type Error = anyhow::Error;

        fn enumerate(
            &mut self,
            _collection: &PimdirCollectionId,
            _request: PimdirEnumerate,
        ) -> Result<PimdirEnumerated, Self::Error> {
            anyhow::bail!("no wire")
        }

        fn fetch(
            &mut self,
            _collection: &PimdirCollectionId,
            _handles: Vec<PimdirHandle>,
            _tier: PimdirTier,
        ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
            anyhow::bail!("no wire")
        }

        fn push(
            &mut self,
            _collection: &PimdirCollectionId,
            _changes: Vec<PimdirChange>,
        ) -> Result<Vec<PimdirPushResult>, Self::Error> {
            anyhow::bail!("no wire")
        }
    }

    /// A message probed at `Meta`: its envelope, no body yet.
    fn probed(collection: &str, handle: &str) -> PimdirWriteOp {
        PimdirWriteOp::UpsertPlacement(PimdirPlacement {
            collection: PimdirCollectionId(collection.into()),
            handle: PimdirHandle(handle.into()),
            link_id: Some(PimdirLinkId(format!("mid:{handle}@example.org"))),
            object: None,
            level: PimdirLevel::Meta,
            summary: None,
            sort_key: PimdirSortKey::default(),
            flags: PimdirFlags::default(),
            status: PimdirStatus::Clean,
            conflict_revision: None,
            conflict_object: None,
            base: None,
            origin: None,
        })
    }

    /// A run stopped while downloading keeps every batch it applied.
    ///
    /// The bodies of an account used to be raised to `Full` only once all of
    /// them had downloaded, so a run killed an hour in kept nothing and the
    /// next one started over. Each batch is now applied as it lands.
    #[test]
    fn an_interrupted_hydration_keeps_the_batches_it_applied() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = PimdirStore::open(dir.path())
            .unwrap()
            .for_account("mail")
            .for_source("imap");
        let handles = ["1", "2", "3", "4", "5", "6"];
        for collection in ["imap/INBOX", "imap/Archive"] {
            store
                .ensure_collection(collection, "message/rfc822")
                .unwrap();
            store
                .write(handles.iter().map(|h| probed(collection, h)).collect())
                .unwrap();
        }

        let batches = vec![
            batch("imap/Archive", &["1", "2"]),
            batch("imap/INBOX", &["1", "2", "3"]),
            batch("imap/Archive", &["3", "4"]),
            batch("imap/INBOX", &["4", "5", "6"]),
        ];

        let fetched = AtomicUsize::new(0);
        let mut workers = [&mut store];
        let outcome = hydrate_pool(&mut workers, batches, |store, batch| {
            if fetched.fetch_add(1, Ordering::Relaxed) == 2 {
                anyhow::bail!("connection lost");
            }
            let collection = &batch.collection;
            let items = batch
                .bodies
                .iter()
                .map(|(handle, _)| {
                    let body = format!(
                        "Message-ID: <{}@example.org>\r\nSubject: {collection}\r\n\r\nhi\r\n",
                        handle.0
                    );
                    let blobs = PimdirStore::open(dir.path()).unwrap().blobs();
                    PimdirFetchedItem {
                        handle: handle.clone(),
                        link_id: PimdirLinkId(format!("mid:{}@example.org", handle.0)),
                        summary: None,
                        sort_key: PimdirSortKey::default(),
                        body: Some(PimdirFetchedBody::Inline {
                            hash: blobs.hash(body.as_bytes()),
                            bytes: body.into_bytes(),
                        }),
                        revision: None,
                    }
                })
                .collect();
            if let Err(err) = apply_batch(store, NoWire, collection, items) {
                warn!("{collection} write error: {err:#}");
            }
            Ok(())
        });

        assert_eq!(
            outcome.unwrap_err().to_string(),
            "connection lost",
            "the run still fails",
        );
        let kept = |collection: &str| -> Vec<String> {
            projection_view(&store, collection, "imap")
                .unwrap()
                .into_iter()
                .filter(|p| p.object.is_some() && p.level == PimdirLevel::Full)
                .map(|p| p.handle.0)
                .collect()
        };
        assert_eq!(kept("imap/Archive"), ["1", "2"]);
        assert_eq!(kept("imap/INBOX"), ["1", "2", "3"]);
        assert_eq!(fetched.load(Ordering::Relaxed), 3, "the pool stopped");
    }

    fn target(handle: &str, size: u64, date: Option<&str>) -> HydrateTarget {
        HydrateTarget {
            handle: PimdirHandle(handle.into()),
            size,
            date: date.map(String::from),
        }
    }

    fn queued(batches: Vec<HydrateBatch>) -> Vec<(String, Vec<String>)> {
        batches
            .into_iter()
            .map(|batch| {
                let handles = batch.bodies.into_iter().map(|(h, _)| h.0).collect();
                (batch.collection, handles)
            })
            .collect()
    }

    /// A batch of `handles` read from `collection`, of no known size.
    fn batch(collection: &str, handles: &[&str]) -> HydrateBatch {
        HydrateBatch {
            collection: collection.to_string(),
            bodies: handles
                .iter()
                .map(|handle| (PimdirHandle((*handle).into()), 0))
                .collect(),
        }
    }

    /// The download order is the caller's: largest first by default, the
    /// fastest whole run, or newest first, so that on a large account today's
    /// mail does not wait behind years of large attachments.
    #[test]
    fn bodies_download_largest_or_newest_first() {
        let plans: Vec<CollectionPlan> = vec![
            (
                "imap/Archive".into(),
                vec![
                    target("a", 900, Some("2019-05-01T08:00:00Z")),
                    target("b", 10, Some("2026-10-03T08:00:00Z")),
                ],
            ),
            (
                "imap/INBOX".into(),
                vec![
                    target("c", 50, Some("2026-10-04T08:00:00Z")),
                    target("d", 20, None),
                    target("e", 500, Some("2025-01-01T08:00:00Z")),
                ],
            ),
        ];
        let owned = |list: &[&str]| list.iter().map(|h| h.to_string()).collect::<Vec<_>>();

        assert_eq!(
            queued(hydrate_batches(
                plans.clone(),
                DownloadOrder::Largest,
                0,
                false
            )),
            [
                ("imap/Archive".to_string(), owned(&["a", "b"])),
                ("imap/INBOX".to_string(), owned(&["e", "c", "d"])),
            ],
        );
        assert_eq!(
            queued(hydrate_batches(
                plans.clone(),
                DownloadOrder::Newest,
                0,
                false
            )),
            [
                ("imap/INBOX".to_string(), owned(&["c", "e", "d"])),
                ("imap/Archive".to_string(), owned(&["b", "a"])),
            ],
            "newest first, a mail without a date last",
        );
    }

    /// Newest first holds across collections batch by batch: an older batch
    /// of the Inbox waits behind a newer one of another folder.
    #[test]
    fn newest_first_interleaves_collections_by_batch() {
        let inbox = (0..BATCH_SIZE + 1)
            .map(|i| {
                let date = format!("2026-10-04T{:02}:{:02}:00Z", i / 60, i % 60);
                target(&i.to_string(), 1, Some(&date))
            })
            .collect();
        let plans: Vec<CollectionPlan> = vec![
            ("imap/INBOX".into(), inbox),
            (
                "imap/Archive".into(),
                vec![target("x", 1, Some("2026-10-04T00:30:00Z"))],
            ),
        ];

        let order: Vec<(String, usize, String)> =
            hydrate_batches(plans.clone(), DownloadOrder::Newest, 0, false)
                .into_iter()
                .map(|batch| {
                    let first = batch.bodies[0].0.0.clone();
                    (batch.collection, batch.bodies.len(), first)
                })
                .collect();
        assert_eq!(
            order,
            [
                ("imap/INBOX".to_string(), BATCH_SIZE, BATCH_SIZE.to_string()),
                ("imap/Archive".to_string(), 1, "x".to_string()),
                ("imap/INBOX".to_string(), 1, "0".to_string()),
            ],
        );
    }

    /// A mail source answering rounds in scripted pages, newest first, its
    /// cursor the index of the next page.
    struct PagedRemote {
        /// Each page's members, as `(handle, Date header)`.
        pages: Vec<Vec<(&'static str, Option<&'static str>)>>,
        /// The page whose request fails once, as a dropped connection would.
        fail_at: Option<usize>,
        /// Whether the next resumed request is refused its cursor.
        reject_cursor: bool,
        /// Every request, in order.
        requests: Vec<PimdirEnumerate>,
    }

    impl PagedRemote {
        fn new(pages: Vec<Vec<(&'static str, Option<&'static str>)>>) -> Self {
            Self {
                pages,
                fail_at: None,
                reject_cursor: false,
                requests: Vec::new(),
            }
        }

        fn cursors(&self) -> Vec<Option<String>> {
            self.requests
                .iter()
                .map(|request| {
                    request
                        .cursor()
                        .map(|cursor| String::from_utf8_lossy(&cursor.0).into_owned())
                })
                .collect()
        }
    }

    impl PimdirRemote for PagedRemote {
        type Error = anyhow::Error;

        fn enumerate(
            &mut self,
            _collection: &PimdirCollectionId,
            request: PimdirEnumerate,
        ) -> Result<PimdirEnumerated, Self::Error> {
            self.requests.push(request.clone());
            let cursor = match &request.listing {
                PimdirListing::Head(_) => return Ok(PimdirEnumerated::NoHead),
                PimdirListing::Delta(checkpoint) => {
                    return Ok(PimdirEnumerated::Page(PimdirRemoteSnapshot::delta(
                        Vec::new(),
                        Vec::new(),
                        checkpoint.clone(),
                    )));
                }
                PimdirListing::Round { cursor, band } => (cursor.clone(), *band),
            };
            let (cursor, band) = cursor;
            if cursor.is_some() && mem::take(&mut self.reject_cursor) {
                return Ok(PimdirEnumerated::CursorRejected);
            }

            let index: usize = cursor
                .map(|cursor| String::from_utf8_lossy(&cursor.0).parse().unwrap())
                .unwrap_or(0);
            if self.fail_at == Some(index) {
                self.fail_at = None;
                anyhow::bail!("connection reset by peer");
            }

            let items = self.pages[index]
                .iter()
                .map(|(handle, date)| {
                    let mut header = format!("Message-ID: <{handle}@example.org>\r\n");
                    if let Some(date) = date {
                        header.push_str(&format!("Date: {date}\r\n"));
                    }
                    PimdirRemoteItem {
                        handle: PimdirHandle((*handle).into()),
                        flags: PimdirFlags::default(),
                        revision: None,
                        meta: io_pimdir::summary::mail::derive_meta(header.as_bytes(), None, None)
                            .into(),
                    }
                })
                .collect();
            let next = (index + 1 < self.pages.len())
                .then(|| PimdirCursor((index + 1).to_string().into_bytes()));
            // NOTE: a band round carries no checkpoint, the one the store
            // holds staying the source's.
            let checkpoint = (index == 0 && !band).then(|| PimdirCheckpoint(b"modseq-1".to_vec()));

            Ok(PimdirEnumerated::Page(PimdirRemoteSnapshot::page(
                items, next, checkpoint,
            )))
        }

        fn fetch(
            &mut self,
            _collection: &PimdirCollectionId,
            _handles: Vec<PimdirHandle>,
            _tier: PimdirTier,
        ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
            anyhow::bail!("every member arrives named")
        }

        fn push(
            &mut self,
            _collection: &PimdirCollectionId,
            changes: Vec<PimdirChange>,
        ) -> Result<Vec<PimdirPushResult>, Self::Error> {
            anyhow::bail!("nothing is staged, so nothing pushes: {changes:?}")
        }
    }

    /// A mailbox holding one message from 2020, synced before any scope.
    fn mailbox_with_an_old_message(dir: &Path) -> PimdirSourceStore {
        let old = io_pimdir::summary::mail::derive_meta(
            b"Message-ID: <old@example.org>\r\nDate: Wed, 01 Jan 2020 10:00:00 +0000\r\n\r\n",
            None,
            None,
        );
        let mut store = PimdirStore::open(dir).unwrap().for_source("imap");
        store
            .ensure_collection("imap/INBOX", "message/rfc822")
            .unwrap();
        store
            .write(vec![PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                collection: PimdirCollectionId("imap/INBOX".into()),
                handle: PimdirHandle("1".into()),
                link_id: Some(old.link_id),
                object: None,
                level: PimdirLevel::Meta,
                summary: old.summary,
                sort_key: old.sort_key,
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
            })])
            .unwrap();
        store
    }

    fn scoped_sync(store: &mut PimdirSourceStore, remote: &mut PagedRemote) -> Result<()> {
        let verb = sync_verb(
            "imap/INBOX",
            true,
            PimdirPushRights::all(),
            PimdirConflictPolicy::Manual,
            PimdirScope::since("2026-09-07T00:00:00Z"),
            false,
        );
        run_verb(store, remote, verb).map(|_| ())
    }

    fn handles_of(store: &PimdirSourceStore) -> BTreeSet<String> {
        load_side(store, "imap/INBOX")
            .unwrap()
            .into_iter()
            .map(|placement| placement.handle.0)
            .collect()
    }

    fn pages() -> Vec<Vec<(&'static str, Option<&'static str>)>> {
        vec![
            vec![
                ("31", Some("Tue, 06 Oct 2026 10:00:00 +0000")),
                ("30", Some("Tue, 01 Jan 2030 10:00:00 +0000")),
            ],
            vec![
                ("20", Some("Mon, 05 Oct 2026 10:00:00 +0000")),
                ("19", None),
            ],
            vec![("10", Some("Fri, 02 Oct 2026 10:00:00 +0000"))],
        ]
    }

    /// A round broken between two pages keeps what landed and resumes from
    /// its cursor; its last page infers no delete out of scope, and nothing
    /// pushes a delete nobody staged.
    #[test]
    fn an_interrupted_round_resumes_from_its_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = mailbox_with_an_old_message(dir.path());
        let mut remote = PagedRemote::new(pages());
        remote.fail_at = Some(1);

        let err = scoped_sync(&mut store, &mut remote).unwrap_err();
        assert!(format!("{err:#}").contains("connection reset"), "{err:#}");
        assert_eq!(
            handles_of(&store),
            names(&["1", "30", "31"]),
            "page one landed"
        );
        let coverage = store.list_coverage("imap/INBOX").unwrap();
        assert_eq!(coverage.len(), 1);
        assert!(coverage[0].coverage.is_none(), "no round closed yet");
        assert!(coverage[0].round.is_some(), "the round is left open");

        scoped_sync(&mut store, &mut remote).unwrap();
        assert_eq!(
            remote.cursors(),
            [
                None,
                Some(String::from("1")),
                Some(String::from("1")),
                Some(String::from("2"))
            ],
            "the second run asked for the page the first never got",
        );
        assert_eq!(
            handles_of(&store),
            names(&["1", "10", "19", "20", "30", "31"]),
            "the 2020 message no page listed lies out of scope and stays",
        );
        let coverage = store.list_coverage("imap/INBOX").unwrap();
        let closed = coverage[0].coverage.as_ref().expect("the round closed");
        assert_eq!(closed.scope, PimdirScope::since("2026-09-07T00:00:00Z"));
        assert!(coverage[0].round.is_none());

        let requests = remote.requests.len();
        scoped_sync(&mut store, &mut remote).unwrap();
        assert!(
            matches!(remote.requests[requests].listing, PimdirListing::Delta(_)),
            "a covered scope lists the delta from the round's checkpoint",
        );
    }

    /// A cursor the source refuses restarts the round from its first page,
    /// the members already landed matched by handle and never twice.
    #[test]
    fn a_refused_cursor_restarts_the_round() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = mailbox_with_an_old_message(dir.path());
        let mut remote = PagedRemote::new(pages());
        remote.fail_at = Some(1);
        scoped_sync(&mut store, &mut remote).unwrap_err();

        remote.reject_cursor = true;
        scoped_sync(&mut store, &mut remote).unwrap();
        assert_eq!(
            remote.cursors(),
            [
                None,
                Some(String::from("1")),
                Some(String::from("1")),
                None,
                Some(String::from("1")),
                Some(String::from("2")),
            ],
        );
        assert_eq!(
            handles_of(&store),
            names(&["1", "10", "19", "20", "30", "31"])
        );
    }

    /// A widening on a source whose checkpoint is bound to no scope (IMAP,
    /// Gmail) lists the band its coverage lacks by a date filter that
    /// misses mail of no usable date: IMAP's `SENTSINCE` a garbled `Date`,
    /// Gmail's `after:` a message with no `Date` received outside the band.
    /// The band round infers no delete of either (io-pimdir `ff28408`).
    #[test]
    fn a_band_round_keeps_the_undated_mail_it_does_not_list() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = mailbox_with_an_old_message(dir.path());
        let mut remote = PagedRemote::new(vec![
            vec![
                ("31", Some("Tue, 06 Oct 2026 10:00:00 +0000")),
                ("20", Some("Mon, 05 Oct 2026 10:00:00 +0000")),
            ],
            vec![("19", None), ("18", Some("sometime last week"))],
        ]);
        scoped_sync(&mut store, &mut remote).unwrap();
        assert_eq!(handles_of(&store), names(&["1", "18", "19", "20", "31"]));

        remote.pages = vec![vec![("5", Some("Mon, 02 Mar 2026 10:00:00 +0000"))]];
        let requests = remote.requests.len();
        let verb = sync_verb(
            "imap/INBOX",
            true,
            PimdirPushRights::all(),
            PimdirConflictPolicy::Manual,
            PimdirScope::since("2026-01-01T00:00:00Z"),
            false,
        );
        run_verb(&mut store, &mut remote, verb).unwrap();

        assert!(
            remote.requests[requests..]
                .iter()
                .all(|request| matches!(request.listing, PimdirListing::Round { band: true, .. })),
            "the widening lists its band alone: {:?}",
            &remote.requests[requests..],
        );
        assert_eq!(
            handles_of(&store),
            names(&["1", "5", "18", "19", "20", "31"]),
            "the band brings its message, the undated and garbled ones stay",
        );
        let coverage = store.list_coverage("imap/INBOX").unwrap();
        let closed = coverage[0].coverage.as_ref().expect("the band closed");
        assert_eq!(closed.scope, PimdirScope::since("2026-01-01T00:00:00Z"));
    }

    /// A pager that hydrates each landed page where the sync stands at rest,
    /// as phase 1's idle connections do, recording what it was told.
    #[derive(Default)]
    struct HydratingPager {
        /// Each call, its landed handles sorted, and whether it resumed.
        told: Vec<(Vec<String>, bool)>,
        /// Whether a landed page gets its bodies.
        hydrate: bool,
        /// Listing waits begun, then ended.
        listing: usize,
        listed: usize,
    }

    impl Pager for HydratingPager {
        fn at_rest(
            &mut self,
            store: &mut PimdirSourceStore,
            collection: &PimdirCollectionId,
            landed: Vec<PimdirHandle>,
            resumed: bool,
            _next: Option<&PimdirListing>,
        ) -> Rest {
            assert_eq!(self.listing, self.listed, "never told mid-listing");
            let mut names: Vec<String> = landed.iter().map(|h| h.0.clone()).collect();
            names.sort();
            self.told.push((names, resumed));
            if landed.is_empty() {
                return Rest::Go;
            }

            let placements = store
                .load(collection, &PimdirLoadScope::Handles(landed.clone()))
                .unwrap()
                .placements;
            assert_eq!(placements.len(), landed.len(), "the page has landed");
            if !self.hydrate {
                return Rest::Go;
            }
            let blobs = store.blobs();
            let items = placements
                .into_iter()
                .map(|placement| {
                    let body = format!(
                        "Message-ID: <{}@example.org>\r\n\r\nhi\r\n",
                        placement.handle.0
                    );
                    PimdirFetchedItem {
                        handle: placement.handle,
                        link_id: placement.link_id.unwrap(),
                        summary: None,
                        sort_key: PimdirSortKey::default(),
                        body: Some(PimdirFetchedBody::Inline {
                            hash: blobs.hash(body.as_bytes()),
                            bytes: body.into_bytes(),
                        }),
                        revision: None,
                    }
                })
                .collect();
            apply_batch(store, NoWire, collection.as_str(), items).unwrap();
            Rest::Go
        }

        fn listing(&mut self) {
            self.listing += 1;
        }

        fn listed(&mut self) {
            self.listed += 1;
        }
    }

    fn paged_sync(
        store: &mut PimdirSourceStore,
        remote: &mut PagedRemote,
        pager: &mut dyn Pager,
    ) -> Result<()> {
        let verb = sync_verb(
            "imap/INBOX",
            false,
            PimdirPushRights::all(),
            PimdirConflictPolicy::Manual,
            PimdirScope::since("2026-09-07T00:00:00Z"),
            false,
        );
        run_verb_paged(store, remote, verb, Some(pager)).map(|_| ())
    }

    fn bodied(store: &PimdirSourceStore) -> BTreeSet<String> {
        load_side(store, "imap/INBOX")
            .unwrap()
            .into_iter()
            .filter(|placement| placement.object.is_some())
            .map(|placement| placement.handle.0)
            .collect()
    }

    /// Each page is told once written, the last one when the sync completes,
    /// and a body applied there survives the pages that land after it, the
    /// last one's whole-collection merge included.
    #[test]
    fn a_landed_page_is_told_at_rest_and_its_bodies_stay() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = mailbox_with_an_old_message(dir.path());
        let mut remote = PagedRemote::new(pages());
        let mut pager = HydratingPager {
            hydrate: true,
            ..Default::default()
        };

        paged_sync(&mut store, &mut remote, &mut pager).unwrap();

        let owned = |list: &[&str]| list.iter().map(|h| h.to_string()).collect::<Vec<_>>();
        assert_eq!(
            pager.told,
            [
                (Vec::new(), false),
                (owned(&["30", "31"]), false),
                (owned(&["19", "20"]), false),
                (owned(&["10"]), false),
            ],
        );
        assert_eq!((pager.listing, pager.listed), (3, 3));
        assert_eq!(
            bodied(&store),
            names(&["10", "19", "20", "30", "31"]),
            "every page kept the bodies applied at rest",
        );
    }

    /// A round resumed is told so before its first page, and its page feed
    /// then queues the bodies the earlier run's pages still owe, newest first
    /// across the account, beside each page landed after.
    #[test]
    fn a_resumed_round_feeds_what_its_earlier_pages_owe() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = mailbox_with_an_old_message(dir.path());
        let mut remote = PagedRemote::new(pages());
        remote.fail_at = Some(1);

        let mut pager = HydratingPager::default();
        paged_sync(&mut store, &mut remote, &mut pager).unwrap_err();
        assert_eq!(pager.told.len(), 2, "page one was told before the failure");
        assert!(bodied(&store).is_empty());

        let mut pager = HydratingPager::default();
        paged_sync(&mut store, &mut remote, &mut pager).unwrap();
        assert_eq!(pager.told[0], (Vec::new(), true), "told once, resumed");
        assert!(pager.told[1..].iter().all(|(_, resumed)| !resumed));

        let dir = tempfile::tempdir().unwrap();
        let mut store = mailbox_with_an_old_message(dir.path());
        let mut remote = PagedRemote::new(pages());
        remote.fail_at = Some(1);
        paged_sync(&mut store, &mut remote, &mut HydratingPager::default()).unwrap_err();

        let gate = Mutex::new(());
        let work = Work::new(DownloadOrder::Newest, 0, false);
        let mut pager = PageFeed {
            feed: Some(&work),
            ..PageFeed::hold(&gate)
        };
        paged_sync(&mut store, &mut remote, &mut pager).unwrap();
        drop(pager);

        let mut fed = Vec::new();
        while let Some(job) = work.next() {
            let Job::Hydrate {
                batch,
                primed: false,
            } = job
            else {
                panic!("only page bodies are queued: {job:?}");
            };
            assert_eq!(batch.collection, "imap/INBOX");
            fed.push(
                batch
                    .bodies
                    .into_iter()
                    .map(|(h, _)| h.0)
                    .collect::<Vec<_>>(),
            );
        }
        assert_eq!(
            fed,
            [vec!["30", "31", "1"], vec!["20", "19"], vec!["10"]],
            "the resumed round's earlier page first, the 2020 mail it owes too",
        );
    }

    /// A connection with no collection left waits on the feed until a page
    /// lands or every spine has ended.
    #[test]
    fn the_feed_waits_while_a_spine_may_still_feed() {
        let work = Work::new(DownloadOrder::Largest, 1, false);
        assert_eq!(
            work.next(),
            Some(Job::Spine {
                index: 0,
                pass: Pass::Whole
            })
        );
        thread::scope(|scope| {
            let waiter = scope.spawn(|| {
                let mut got = Vec::new();
                while let Some(Job::Hydrate { batch, .. }) = work.next() {
                    got.push(batch.bodies.len());
                }
                got
            });
            thread::sleep(std::time::Duration::from_millis(20));
            work.feed(
                "imap/INBOX",
                0,
                vec![target("a", 1, None), target("b", 9, None)],
                false,
            );
            thread::sleep(std::time::Duration::from_millis(20));
            work.spined(Pass::Whole, 0, false);
            assert_eq!(waiter.join().unwrap(), [2]);
        });
    }

    /// Runs a scoped sync of the INBOX under `pager`, answering whether it
    /// paused.
    fn paused_sync(
        store: &mut PimdirSourceStore,
        remote: &mut PagedRemote,
        pager: &mut dyn Pager,
    ) -> bool {
        let verb = sync_verb(
            "imap/INBOX",
            false,
            PimdirPushRights::all(),
            PimdirConflictPolicy::Manual,
            PimdirScope::since("2026-09-07T00:00:00Z"),
            false,
        );
        match run_verb_paged(store, remote, verb, Some(pager)).unwrap() {
            Paged::Done(_) => false,
            Paged::Paused(_) => true,
        }
    }

    /// A pager pausing a round leaves the store as a run killed after its
    /// first page does (see `an_interrupted_round_resumes_from_its_cursor`):
    /// the page landed, the round open at its cursor. A round resumed is
    /// paused before it asks anything, and the next sync left to page
    /// resumes from the cursor and closes it; a delta is never paused.
    #[test]
    fn a_paused_round_stands_where_a_killed_one_would() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = mailbox_with_an_old_message(dir.path());
        let mut remote = PagedRemote::new(pages());
        let gate = Mutex::new(());
        let pausing = |gate| PageFeed {
            pause: true,
            ..PageFeed::hold(gate)
        };

        assert!(paused_sync(&mut store, &mut remote, &mut pausing(&gate)));
        assert_eq!(remote.cursors(), [None], "the first page alone");
        assert_eq!(handles_of(&store), names(&["1", "30", "31"]));
        let coverage = store.list_coverage("imap/INBOX").unwrap();
        assert!(coverage[0].coverage.is_none(), "no round closed yet");
        assert!(coverage[0].round.is_some(), "the round is left open");

        assert!(paused_sync(&mut store, &mut remote, &mut pausing(&gate)));
        assert_eq!(remote.cursors(), [None], "a resumed round asks nothing");

        assert!(!paused_sync(
            &mut store,
            &mut remote,
            &mut PageFeed::hold(&gate)
        ));
        assert_eq!(
            remote.cursors(),
            [None, Some(String::from("1")), Some(String::from("2"))],
            "the fill resumes from the cursor the first page left",
        );
        assert_eq!(
            handles_of(&store),
            names(&["1", "10", "19", "20", "30", "31"])
        );
        let coverage = store.list_coverage("imap/INBOX").unwrap();
        assert!(coverage[0].coverage.is_some(), "the round closed");
        assert!(coverage[0].round.is_none());

        let requests = remote.requests.len();
        assert!(!paused_sync(&mut store, &mut remote, &mut pausing(&gate)));
        assert!(
            matches!(remote.requests[requests].listing, PimdirListing::Delta(_)),
            "a whole collection runs its delta to the end",
        );
    }

    /// Takes the next job, failing on none.
    fn job(work: &Work) -> Job {
        work.next().expect("a job")
    }

    fn spine_job(index: usize, pass: Pass) -> Job {
        Job::Spine { index, pass }
    }

    /// Priming lists the first page of every collection, in the run's order,
    /// each followed by its newest bodies, before any collection lists a
    /// second page; the fill waits for the last primed body too, other
    /// bodies downloading meanwhile.
    #[test]
    fn every_first_page_and_its_newest_bodies_come_before_any_second_page() {
        let work = Work::new(DownloadOrder::Newest, 3, true);
        let primed = |collection: &str, handle: &str| Job::Hydrate {
            batch: HydrateBatch {
                collection: collection.to_string(),
                bodies: vec![(PimdirHandle(handle.into()), 1)],
            },
            primed: true,
        };

        assert_eq!(job(&work), spine_job(0, Pass::First));
        work.feed("INBOX", 0, vec![target("i1", 1, None)], true);
        work.spined(Pass::First, 0, true);
        assert_eq!(job(&work), primed("INBOX", "i1"), "its bodies right after");
        work.downloaded(true);

        assert_eq!(job(&work), spine_job(1, Pass::First));
        work.spined(Pass::First, 1, false);
        assert_eq!(job(&work), spine_job(2, Pass::First));
        work.feed("Trash", 2, vec![target("t1", 1, None)], true);
        work.feed("Sent", 1, vec![target("s1", 1, None)], false);
        work.spined(Pass::First, 2, true);
        assert_eq!(job(&work), primed("Trash", "t1"));

        assert!(
            matches!(job(&work), Job::Hydrate { primed: false, .. }),
            "a primed body under way holds the fill back, not the rest",
        );
        work.downloaded(false);
        work.downloaded(true);
        assert_eq!(job(&work), spine_job(0, Pass::Fill), "the fill, in order");
        assert_eq!(job(&work), spine_job(2, Pass::Fill));
        work.spined(Pass::Fill, 0, false);
        work.spined(Pass::Fill, 2, false);
        assert_eq!(work.next(), None);
    }

    /// Several connections take the first pass together, and no fill
    /// starts while one of its spines is under way.
    #[test]
    fn the_fill_waits_for_every_first_page() {
        let work = Work::new(DownloadOrder::Newest, 2, true);
        assert_eq!(job(&work), spine_job(0, Pass::First));
        assert_eq!(job(&work), spine_job(1, Pass::First));
        work.spined(Pass::First, 0, true);
        thread::scope(|scope| {
            let filler = scope.spawn(|| job(&work));
            thread::sleep(std::time::Duration::from_millis(20));
            assert!(
                !filler.is_finished(),
                "page 2 of one waits for page 1 of all"
            );
            work.spined(Pass::First, 1, false);
            assert_eq!(filler.join().unwrap(), spine_job(0, Pass::Fill));
        });
        work.spined(Pass::Fill, 0, false);
        assert_eq!(work.next(), None);
    }

    /// Queued bodies leave in the download order, a tie going to the
    /// collection the run takes first, whichever was queued first.
    #[test]
    fn a_tie_between_batches_goes_to_the_collection_taken_first() {
        let work = Work::new(DownloadOrder::Newest, 0, false);
        let date = Some("2026-10-04T08:00:00Z");
        work.feed("Archive", 3, vec![target("a", 1, date)], false);
        work.feed("INBOX", 0, vec![target("i", 1, date)], false);
        work.feed(
            "Sent",
            1,
            vec![target("s", 1, Some("2026-10-05T08:00:00Z"))],
            false,
        );
        let mut order = Vec::new();
        while let Some(Job::Hydrate { batch, .. }) = work.next() {
            order.push(batch.collection);
        }
        assert_eq!(order, ["Sent", "INBOX", "Archive"]);
    }

    /// Phase 2 takes each collection's newest bodies ahead of the rest,
    /// whatever the download order, then the rest in it.
    #[test]
    fn phase_two_downloads_the_newest_of_each_collection_first() {
        let plans: Vec<CollectionPlan> = vec![
            (
                "imap/INBOX".into(),
                vec![
                    target("old", 900, Some("2019-05-01T08:00:00Z")),
                    target("new", 10, Some("2026-10-03T08:00:00Z")),
                ],
            ),
            (
                "imap/Archive".into(),
                vec![
                    target("big", 800, Some("2020-01-01T08:00:00Z")),
                    target("recent", 5, Some("2026-10-01T08:00:00Z")),
                ],
            ),
        ];
        let owned = |list: &[&str]| list.iter().map(|h| h.to_string()).collect::<Vec<_>>();
        assert_eq!(
            queued(hydrate_batches(
                plans.clone(),
                DownloadOrder::Largest,
                1,
                false
            )),
            [
                ("imap/INBOX".to_string(), owned(&["new"])),
                ("imap/Archive".to_string(), owned(&["recent"])),
                ("imap/INBOX".to_string(), owned(&["old"])),
                ("imap/Archive".to_string(), owned(&["big"])),
            ],
        );
    }

    /// The run takes the inbox first and the trash last, every collection
    /// without a role in between by name.
    #[test]
    fn collections_are_taken_by_role_then_by_name() {
        let ids = [
            "Trash", "zeta", "INBOX", "Spam", "Archive", "Sent", "Drafts", "All", "beta",
        ];
        let kept: BTreeSet<String> = ids.iter().map(|id| id.to_string()).collect();
        let names: BTreeMap<String, String> = ids
            .iter()
            .map(|id| (id.to_string(), id.to_string()))
            .collect();
        let roles: BTreeMap<String, String> = [
            ("INBOX", "inbox"),
            ("Sent", "sent"),
            ("Drafts", "drafts"),
            ("Archive", "archive"),
            ("All", "all"),
            ("Spam", "junk"),
            ("Trash", "trash"),
        ]
        .iter()
        .map(|(id, role)| (id.to_string(), role.to_string()))
        .collect();

        assert_eq!(
            run_order(&kept, &names, &roles),
            [
                "INBOX", "Sent", "Drafts", "Archive", "beta", "zeta", "All", "Spam", "Trash"
            ],
        );
    }

    /// Under `--bodies listed`, only what a page of this run landed is
    /// downloaded, the rest staying owed.
    #[test]
    fn listed_bodies_are_those_this_run_landed() {
        let listed = Listed::default();
        listed.record("imap/INBOX", &[PimdirHandle("2".into())]);
        let kept = listed.keep(
            "imap/INBOX",
            vec![target("1", 1, None), target("2", 1, None)],
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].handle.0, "2");
        assert!(
            listed
                .keep("imap/Archive", vec![target("2", 1, None)])
                .is_empty()
        );
    }

    fn source_of(toml: &str, name: &str) -> SourceConfig {
        let config: AccountConfig = toml::from_str(toml).unwrap();
        config.sources().unwrap()[name].clone()
    }

    /// IMAP opens the run's connections, Graph three unless its pool size
    /// says otherwise and never more than the run's, Google and DAV one.
    #[test]
    fn each_backend_opens_its_own_connection_budget() {
        let imap = source_of("imap.server = \"imaps://localhost\"", "imap");
        assert_eq!(connection_budget(&imap, 4), 4);
        assert_eq!(connection_budget(&imap, 8), 8);

        let graph = "msgraph.auth.token.raw = \"x\"";
        let msgraph = source_of(graph, "msgraph");
        assert_eq!(connection_budget(&msgraph, 4), 3);
        assert_eq!(connection_budget(&msgraph, 1), 1, "--connections 1");
        let sized = source_of(&format!("{graph}\nmsgraph.pool-size = 2"), "msgraph");
        assert_eq!(connection_budget(&sized, 4), 2);
        let calendar = source_of(
            "msgraph-calendar.auth.token.raw = \"x\"",
            "msgraph-calendar",
        );
        assert_eq!(connection_budget(&calendar, 4), 3);

        let gmail = source_of("gmail.auth.token.raw = \"x\"", "gmail");
        assert_eq!(connection_budget(&gmail, 4), 1);
        let caldav = source_of(
            "caldav.server = \"https://localhost\"\ncaldav.auth.basic.username = \"u\"\ncaldav.auth.basic.password.raw = \"p\"",
            "caldav",
        );
        assert_eq!(connection_budget(&caldav, 4), 1);
    }

    /// Priming is on under the order a caller picks to show mail early,
    /// off under the fastest whole run, and the caller may say otherwise.
    #[test]
    fn priming_defaults_by_the_download_order() {
        let wait = throttle::MAX_WAIT;
        let pacing = |order, prime| Pacing::new(order, prime, Bodies::All, wait).prime;
        assert_eq!(pacing(DownloadOrder::Newest, None), DEFAULT_PRIME);
        assert_eq!(pacing(DownloadOrder::Largest, None), 0);
        assert_eq!(pacing(DownloadOrder::Newest, Some(0)), 0);
        assert_eq!(pacing(DownloadOrder::Largest, Some(10)), 10);
    }
}

/// Several Microsoft Graph connections of one source against a fake Graph
/// answering on every connection at once: the run's budget is what runs
/// side by side, and one throttle stops them all.
#[cfg(all(test, feature = "msgraph"))]
mod graph_budget {
    use std::{
        collections::VecDeque,
        io::{self, Read, Write},
        sync::{
            Arc, Mutex, OnceLock,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
        time::{Duration, Instant, SystemTime},
    };

    use base64::{Engine, engine::general_purpose::STANDARD};
    use crossbeam_queue::SegQueue;
    use io_msgraph::v1::client::MsgraphStream;
    use io_pimdir::{client::PimdirStore, placement::PimdirHandle};

    use super::{HydrateBatch, connection_budget, hydrate_pool};
    use crate::{
        client::Client,
        config::AccountConfig,
        kind::Kind,
        msgraph::client::{GraphClient, SizeHints},
        offline::remote::hydrate_batch,
        throttle::Throttle,
    };

    /// How long the fake serves a request: long enough for the
    /// connections' requests to overlap.
    const SERVE: Duration = Duration::from_millis(30);

    /// The fake Graph: every request it served, and how many at once.
    #[derive(Default)]
    struct Graph {
        /// The message whose get is throttled, and the wait stated, in
        /// seconds, ten minutes by default.
        hot: Option<String>,
        retry_after: Option<&'static str>,
        connections: AtomicUsize,
        serving: AtomicUsize,
        most: AtomicUsize,
        /// Each request: its connection, when it reached the server, and
        /// the ids it asked.
        served: Mutex<Vec<(usize, Instant, Vec<String>)>>,
        /// When the throttled answer left.
        throttled: Mutex<Option<Instant>>,
        /// The source's throttle, when a test watches it: a request
        /// reaching the server after the throttled answer left is answered
        /// only once the source gave up, so what a connection does after
        /// the give-up does not hang on how the threads interleave.
        gate: OnceLock<Arc<Throttle>>,
    }

    fn raw(id: &str) -> Vec<u8> {
        format!("Message-ID: <{id}@example.org>\r\nSubject: {id}\r\n\r\nbody of {id}\r\n")
            .into_bytes()
    }

    fn reply(status: u16, body: &[u8], content_type: &str) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body);
        out
    }

    impl Graph {
        /// Answers one request, `line` its request line.
        fn serve(&self, connection: usize, line: &str, body: &[u8]) -> Vec<u8> {
            // NOTE: read under the lock the throttled answer is noted
            // under, so a request is after it exactly when it reached the
            // server once that answer had left.
            let (at, late) = {
                let throttled = self.throttled.lock().unwrap();
                (Instant::now(), throttled.is_some())
            };
            if let Some(throttle) = self.gate.get().filter(|_| late) {
                let deadline = at + Duration::from_secs(10);
                while throttle.gave_up().is_none() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(1));
                }
            }

            let now = self.serving.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            thread::sleep(SERVE);

            let id_of = |url: &str| {
                url.split("messages/")
                    .nth(1)
                    .and_then(|rest| rest.split('/').next())
                    .unwrap_or_default()
                    .to_owned()
            };
            let (asked, answer) = if line.starts_with("POST ") && line.contains("$batch") {
                let batch: serde_json::Value = serde_json::from_slice(body).unwrap();
                let mut asked = Vec::new();
                let mut responses = Vec::new();
                for request in batch["requests"].as_array().unwrap() {
                    let id = id_of(request["url"].as_str().unwrap());
                    let rid = request["id"].clone();
                    if self.hot.as_deref() == Some(id.as_str()) {
                        *self.throttled.lock().unwrap() = Some(Instant::now());
                        responses.push(serde_json::json!({
                            "id": rid,
                            "status": 429,
                            "headers": { "retry-after": self.retry_after.unwrap_or("600") },
                            "body": { "error": { "code": "TooManyRequests", "message": "slow" } },
                        }));
                    } else {
                        responses.push(serde_json::json!({
                            "id": rid,
                            "status": 200,
                            "body": STANDARD.encode(raw(&id)),
                        }));
                    }
                    asked.push(id);
                }
                let json = serde_json::json!({ "responses": responses });
                let json = serde_json::to_vec(&json).unwrap();
                (asked, reply(200, &json, "application/json"))
            } else {
                let id = id_of(line);
                (vec![id.clone()], reply(200, &raw(&id), "text/plain"))
            };

            self.served.lock().unwrap().push((connection, at, asked));
            self.serving.fetch_sub(1, Ordering::SeqCst);
            answer
        }

        /// Opens a connection to the fake.
        fn connect(self: &Arc<Self>) -> anyhow::Result<Box<dyn MsgraphStream>> {
            let index = self.connections.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(Wire {
                graph: Arc::clone(self),
                index,
                input: Vec::new(),
                out: VecDeque::new(),
            }))
        }
    }

    /// One connection: what the client wrote, served once it reads.
    struct Wire {
        graph: Arc<Graph>,
        index: usize,
        input: Vec<u8>,
        out: VecDeque<u8>,
    }

    impl Wire {
        /// Serves every whole request written so far.
        fn serve(&mut self) {
            while let Some(end) = self.input.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&self.input[..end]).into_owned();
                let length: usize = head
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().ok())?
                    })
                    .unwrap_or(0);
                let start = end + 4;
                if self.input.len() < start + length {
                    return;
                }
                let body: Vec<u8> = self.input[start..start + length].to_vec();
                self.input.drain(..start + length);
                let line = head.lines().next().unwrap_or_default().to_owned();
                let answer = self.graph.serve(self.index, &line, &body);
                self.out.extend(answer);
            }
        }
    }

    impl Read for Wire {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.out.is_empty() {
                self.serve();
            }
            let n = buf.len().min(self.out.len());
            for (slot, byte) in buf.iter_mut().zip(self.out.drain(..n)) {
                *slot = byte;
            }
            Ok(n)
        }
    }

    impl Write for Wire {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.input.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// The budget a Graph mail source takes in a run of four connections,
    /// its clients over the fake, sharing one throttle and the sizes the
    /// listing stated for `ids`.
    fn clients(graph: &Arc<Graph>, ids: &[String]) -> (Vec<Client>, Arc<Throttle>) {
        clients_waiting(graph, ids, crate::throttle::MAX_WAIT)
    }

    fn clients_waiting(
        graph: &Arc<Graph>,
        ids: &[String],
        max_wait: Duration,
    ) -> (Vec<Client>, Arc<Throttle>) {
        let config: AccountConfig = toml::from_str("msgraph.auth.token.raw = \"x\"").unwrap();
        let budget = connection_budget(&config.sources().unwrap()["msgraph"], 4);
        let throttle = Arc::new(Throttle::new("msgraph").waiting(max_wait));
        let sizes = SizeHints::default();
        sizes.noted(ids.iter().map(|id| (id.clone(), 1_000)));
        let clients = (0..budget)
            .map(|_| {
                let graph = Arc::clone(graph);
                let client = GraphClient::over_streams(
                    move || graph.connect(),
                    sizes.clone(),
                    Arc::clone(&throttle),
                )
                .unwrap();
                Client::Msgraph(Box::new(client))
            })
            .collect();
        (clients, throttle)
    }

    fn ids(count: usize) -> Vec<String> {
        (0..count).map(|n| format!("m{n:03}")).collect()
    }

    /// Batches of `size` handles over `ids`.
    fn batches(ids: &[String], size: usize) -> Vec<HydrateBatch> {
        ids.chunks(size)
            .map(|chunk| {
                let bodies = chunk
                    .iter()
                    .map(|id| (PimdirHandle(id.clone()), 0))
                    .collect();
                HydrateBatch {
                    collection: String::from("Inbox"),
                    bodies,
                }
            })
            .collect()
    }

    /// The handles of `batch`.
    fn handles_of_batch(batch: &HydrateBatch) -> Vec<PimdirHandle> {
        batch
            .bodies
            .iter()
            .map(|(handle, _)| handle.clone())
            .collect()
    }

    /// A Graph source downloads over its three connections side by side,
    /// never more, through phase 2's pool, each body asked once.
    #[test]
    fn a_graph_source_runs_its_budget_side_by_side_and_no_more() {
        let graph = Arc::new(Graph::default());
        let ids = ids(60);
        let (mut clients, _) = clients(&graph, &ids);
        assert_eq!(clients.len(), 3, "Graph's default budget");
        let dir = tempfile::tempdir().unwrap();
        let blobs = PimdirStore::open(dir.path()).unwrap().blobs();
        let fetched = AtomicUsize::new(0);

        hydrate_pool(&mut clients, batches(&ids, 5), |client, batch| {
            let handles = handles_of_batch(&batch);
            let hydrated = hydrate_batch(
                Kind::Mail,
                client,
                &batch.collection,
                &handles,
                &|_| None,
                &blobs,
                None,
            )?;
            fetched.fetch_add(hydrated.items.len(), Ordering::Relaxed);
            Ok(())
        })
        .unwrap();

        assert_eq!(fetched.into_inner(), 60);
        assert_eq!(graph.connections.load(Ordering::SeqCst), 3);
        assert_eq!(
            graph.most.load(Ordering::SeqCst),
            3,
            "three at once, never four"
        );
        let served = graph.served.lock().unwrap();
        let mut asked: Vec<&String> = served.iter().flat_map(|(_, _, ids)| ids).collect();
        asked.sort();
        asked.dedup();
        assert_eq!(asked.len(), 60, "each body asked once");
        assert_eq!(
            served.iter().map(|(_, _, ids)| ids.len()).sum::<usize>(),
            60
        );
        for connection in 0..3 {
            assert!(
                served.iter().any(|(c, _, _)| *c == connection),
                "connection {connection} served"
            );
        }
    }

    /// One connection's batch throttled for ten minutes gives the source up,
    /// and no connection sends anything after: those with batches left
    /// fail them at once, as phase 1's idle connections go on to the next.
    ///
    /// A connection that read the throttle before the source gave up may
    /// have one request on its way when the throttled answer leaves; the
    /// fake answers such a request only once the source gave up, so a
    /// connection sending a second one would have sent it after the give-up.
    #[test]
    fn a_throttle_on_one_connection_stops_every_connection() {
        let graph = Arc::new(Graph {
            hot: Some(String::from("m012")),
            ..Default::default()
        });
        let ids = ids(60);
        let (mut clients, throttle) = clients(&graph, &ids);
        assert!(graph.gate.set(Arc::clone(&throttle)).is_ok());
        let dir = tempfile::tempdir().unwrap();
        let blobs = PimdirStore::open(dir.path()).unwrap().blobs();
        let queue = SegQueue::new();
        for batch in batches(&ids, 5) {
            queue.push(batch);
        }
        let failed = AtomicUsize::new(0);

        thread::scope(|scope| {
            for client in clients.iter_mut() {
                let (queue, blobs, failed) = (&queue, &blobs, &failed);
                scope.spawn(move || {
                    while let Some(batch) = queue.pop() {
                        let handles = handles_of_batch(&batch);
                        let collection = &batch.collection;
                        if hydrate_batch(
                            Kind::Mail,
                            client,
                            collection,
                            &handles,
                            &|_| None,
                            blobs,
                            None,
                        )
                        .is_err()
                        {
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
        });

        let until = throttle.gave_up().expect("the source gave up");
        assert!(until > SystemTime::now() + Duration::from_secs(500));
        let throttled = graph.throttled.lock().unwrap().expect("a throttled answer");
        let served = graph.served.lock().unwrap();
        let after: Vec<_> = served.iter().filter(|(_, at, _)| *at > throttled).collect();
        for connection in 0..graph.connections.load(Ordering::SeqCst) {
            assert!(
                after.iter().filter(|(c, _, _)| *c == connection).count() <= 1,
                "connection {connection} sent again after the give-up: {after:?}"
            );
        }
        assert!(
            served.len() < 12,
            "the batches left were not sent: {} of 12",
            served.len()
        );
        assert!(failed.into_inner() > 0, "the batches left failed at once");
    }

    /// `--max-wait` bounds a batch's own waits: a get Graph asks to wait
    /// three seconds, in a run waiting at most one, gives the source up
    /// rather than being sent again.
    #[test]
    fn the_run_s_longest_wait_bounds_a_graph_batch() {
        let graph = Arc::new(Graph {
            hot: Some(String::from("m001")),
            retry_after: Some("3"),
            ..Default::default()
        });
        let ids = ids(5);
        let (mut clients, throttle) = clients_waiting(&graph, &ids, Duration::from_secs(1));
        let dir = tempfile::tempdir().unwrap();
        let blobs = PimdirStore::open(dir.path()).unwrap().blobs();

        let start = Instant::now();
        let handles: Vec<PimdirHandle> = ids.iter().map(|id| PimdirHandle(id.clone())).collect();
        let _ = hydrate_batch(
            Kind::Mail,
            &mut clients[0],
            "Inbox",
            &handles,
            &|_| None,
            &blobs,
            None,
        );
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "no three-second wait"
        );
        assert!(throttle.gave_up().is_some(), "the source gave up");
        assert_eq!(graph.served.lock().unwrap().len(), 1, "never sent again");
    }
}

/// Hydration of a Gmail source, whose message ids name a message across
/// its labels, over scripted sessions: a message under two labels is
/// downloaded once, a body gone is owed nothing more for the run, and the
/// sizes the store records plan the reads.
#[cfg(all(test, feature = "gmail"))]
mod gmail_bodies {
    use std::{
        path::Path,
        sync::{Arc, atomic::AtomicU64},
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
        Authority, GoneBodies, Pacing, SourceCtx, hydrate_plans, open_store, phase2_hydrate,
        projection_view,
    };
    use crate::{
        account::{GmailAccount, SourceAccount, SourceAccountBackend},
        client::{Client, Pool},
        config::AccountConfig,
        gmail::{
            client::scripted::{Wire, batch_answer, scripted},
            run::GmailRun,
        },
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
    pub(super) fn connection() -> (SourceCtx, Wire, Wire) {
        let (client, api, batch) = scripted();
        let config: AccountConfig = toml::from_str("gmail.auth.token.raw = \"x\"").unwrap();
        let perms = config.sources().unwrap()["gmail"].permissions();
        let account = SourceAccount {
            backend: SourceAccountBackend::Gmail(GmailAccount {
                token: SecretString::from(String::from("token")),
                user_id: String::from("me"),
                tls: Tls::default(),
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
        let plans = hydrate_plans(store, "gmail", collections, None, gone).unwrap();
        let blobs = store.blobs();
        phase2_hydrate(
            "gmail",
            plans,
            &Pacing::default(),
            std::slice::from_mut(ctx),
            std::slice::from_mut(store),
            &blobs,
            Some(collections),
            gone,
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

    /// A message under two labels is downloaded once and lands under both;
    /// one gone since it was listed leaves the run complete, is asked once,
    /// and no later batch of the run asks for it again. The sizes come from
    /// the store: no minimal read asks for them.
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

        let requests = batch.requests();
        assert_eq!(requests.len(), 2, "m1 and m2, then m3: {requests:?}");
        assert!(api.requests().is_empty(), "nothing read alone");
        let inbox = bodies(&store, INBOX);
        let work = bodies(&store, WORK);
        assert!(inbox.iter().all(|(_, body)| body.is_some()), "{inbox:?}");
        assert_eq!(work[0].0, "m1");
        assert_eq!(work[0].1, inbox[0].1, "one body for both labels");
        assert_eq!(work[1], (String::from("m3"), None), "m3 stays as listed");

        // NOTE: a second pass of the run plans nothing: m3 is owed nothing
        // more, and the wire, answering no more, would fail any request.
        hydrate(&mut store, &mut ctx, &collections, &run);
        assert_eq!(batch.requests().len(), 2, "nothing asked again");
        assert_eq!(batch.left(), 0);
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
        assert_eq!(batch.requests().len(), 1);

        store.write(vec![owed(WORK, "m1", 40)]).unwrap();
        let collections = vec![INBOX.to_string(), WORK.to_string()];
        hydrate(&mut store, &mut ctx, &collections, &GoneBodies::default());

        assert_eq!(batch.requests().len(), 1, "no second download");
        let inbox = bodies(&store, INBOX);
        let work = bodies(&store, WORK);
        assert!(work[0].1.is_some(), "{work:?}");
        assert_eq!(work[0].1, inbox[0].1);
    }
}

/// Heads of a Gmail round (pimdir SYNC §4, §5), over scripted sessions: a
/// run resuming an open round lists the label's history since the round
/// began before its next page, so what arrives during a long round shows
/// at once.
#[cfg(all(test, feature = "gmail"))]
mod gmail_heads {
    use std::{collections::BTreeSet, path::Path};

    use io_pimdir::{
        client::PimdirSourceStore,
        collection::{PimdirCheckpoint, PimdirCollectionId},
        load::{PimdirLoadScope, PimdirLoaded},
        placement::PimdirHandle,
        remote::PimdirListing,
    };

    use super::{gmail_bodies::connection, open_store, projection_view, sync_side};
    use crate::{
        gmail::client::scripted::{Wire, batch_answer, json, ok},
        offline::{Pager, Rest},
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
                &PimdirLoadScope::All,
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
        script: impl FnOnce(&Wire, &Wire),
    ) -> (bool, Vec<Rested>, Vec<String>) {
        let (mut ctx, api, batch) = connection();
        script(&api, &batch);
        let mut watch = Watch {
            pause,
            rests: Vec::new(),
        };
        let blobs = store.blobs();
        let (_, paused) =
            sync_side(INBOX, &mut ctx, store, &blobs, false, Some(&mut watch)).expect("the sync");
        assert_eq!(
            api.left() + batch.left(),
            0,
            "every answer asked for: {:?} {:?}",
            api.requests(),
            batch.requests()
        );
        (paused, watch.rests, api.requests())
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

    /// A message arriving while the round is open lands at the next run,
    /// before the round's next page, from the history since the round
    /// began; it survives the round's close, which takes the head's
    /// history id as the source's checkpoint, the next delta listing from
    /// it (pimdir vector 62).
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
            // NOTE: the flag listings of the bound m1 and m2.
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
}

/// Band heads of a Graph mail round (pimdir SYNC §4, §5), through the
/// driver over a scripted Graph: a run resuming an open round lists what
/// Graph received since two days before the round began, before its next
/// page, and the round's last page lists that again after the delta link,
/// so an arrival shows at once and survives the round's close.
#[cfg(all(test, feature = "msgraph"))]
mod graph_heads {
    use std::{
        collections::BTreeSet,
        path::Path,
        sync::{Arc, atomic::AtomicU64},
    };

    use chrono::{DateTime, SecondsFormat, TimeDelta};
    use io_pimdir::{
        client::PimdirSourceStore,
        collection::{PimdirCollectionId, PimdirScope},
        load::{PimdirLoadScope, PimdirLoaded},
        placement::PimdirHandle,
        remote::PimdirListing,
    };
    use pimalaya_stream::tls::Tls;
    use secrecy::SecretString;

    use super::{Authority, SourceCtx, open_store, projection_view, sync_side};
    use crate::{
        account::{MsgraphAccount, SourceAccount, SourceAccountBackend},
        client::{Client, Pool},
        config::AccountConfig,
        msgraph::client::{
            GraphClient, GraphKind, SizeHints,
            scripted::{Server, Step},
        },
        offline::{Pager, Rest},
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
                &PimdirLoadScope::All,
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
        let blobs = store.blobs();
        let (_, paused) =
            sync_side(INBOX, &mut ctx, store, &blobs, false, Some(&mut watch)).expect("the sync");
        (paused, watch.rests, requests(&server))
    }

    fn requests(server: &Server) -> Vec<String> {
        server.requests().into_iter().flatten().collect()
    }

    fn context(client: GraphClient) -> SourceCtx {
        let config: AccountConfig = toml::from_str("msgraph.auth.token.raw = \"x\"").unwrap();
        let perms = config.sources().unwrap()["msgraph"].permissions();
        let account = SourceAccount {
            backend: SourceAccountBackend::Msgraph(MsgraphAccount {
                kind: GraphKind::Mail,
                token: SecretString::from(String::from("token")),
                user_id: String::from("me"),
                tls: Tls::default(),
                sizes: SizeHints::default(),
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
}
