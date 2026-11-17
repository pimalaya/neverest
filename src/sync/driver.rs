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
//! A collection one endpoint lists and the other does not is created on the
//! other, unless the store knows both held it: then it was deleted, and the
//! delete crosses only where the other allows collection deletes, which it
//! does not by default.
//!
//! The itemized report lists the cross-side copies, flags and deletes a run
//! plans, plus what its opening pull-only round found the servers had done.

mod collections;
mod hydrate;
mod itemize;
mod local;
mod pair;
mod queue;
mod reconcile;
mod relay;
mod send;
mod spine;
mod upkeep;

use std::{
    cmp::Ordering as CmpOrdering,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    fmt, fs, mem,
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
        PimdirError, PimdirRunError, PimdirSourceStore, PimdirStore,
        blobs::PimdirBlobStage,
        modes,
        producer::{PimdirActionStatus, PimdirProducer},
        reader::PimdirReader,
    },
    codec::PimdirAction,
    collection::{PimdirCollectionId, PimdirScope},
    hub::{PimdirBinding, PimdirSourceId},
    intent::PimdirCollectionCreate,
    load::PimdirLoadSelection,
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
        PimdirConflictPolicy, PimdirParked, PimdirPushRights, PimdirSync, PimdirSyncEvent,
        PimdirSyncOptions, PimdirSyncReport, PimdirWaitReason, PimdirWaiting,
    },
    upgrade::{PimdirUpgrade, PimdirUpgradeOptions, PimdirUpgradeReport},
};
use log::{debug, info, warn};
use pimalaya_cli::error::ItemError;

#[cfg(feature = "msgraph")]
use crate::msgraph::client::GraphKind;
use crate::{
    account::{Account, SourceAccount},
    client::{Client, Pool, Renamed, WrittenItem},
    collection::Collection,
    config::{
        AccountConfig,
        filter::CollectionFilter,
        mode::AccountMode,
        permissions::{CollectionPermissions, SourcePermissions},
        source::{SourceBackendConfig, SourceConfig},
    },
    conflict::merge::Merged,
    error::{Failure, layout_error, pimdir_error},
    flag::Flag,
    kind::{Kind, LinkId},
    sync::{
        Paged, Pager, Rest, capability,
        collection::{self, CollectionIntent},
        driver::{
            collections::*, hydrate::*, itemize::*, local::*, pair::*, queue::*, reconcile::*,
            relay::*, send::*, spine::*, upkeep::*,
        },
        event::SyncEvent,
        hunk::{CollectionHunk, ItemHunk},
        invitation::{
            self, Failure as InvitationFailure, Invitation, InvitationIntent, Refusal, Target,
        },
        progress::{PercentGate, Progress},
        remote::{
            BATCH_SIZE, Bound, CachedFetchRemote, PimRemote, RefusedCreate, RejectedPush,
            body_bytes, hydrate_batch, resolve_kind, stored_item, wire_name,
        },
        report::{
            CollectionCoverage, DisagreedMember, DrainedQueue, FailedStep, ForbiddenWrites,
            IntentEntry, ItemConflict, OpenRound, OverruledBody, ParkedAbsence, ParkedQueueAction,
            PatchEntry, PurgedItems, RefusedChange, RefusedDuplicate, RejectedWrite,
            SourceDownload, StepFailure, SubmitEntry, SyncOutput, ThrottledSource,
            UnansweredPushes, UnlistedMembers, UnreachedEndpoint, UnreadableBodies, WaitReason,
            WaitingCreate,
        },
        run_verb, run_verb_paged, source_id,
        state::{AccountState, LookedUpRoles, StoreState},
        storage::{HydrationSide, hydration_targets, load_side, projection_view, register_syncing},
        submit,
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

/// Resolves `<state base>/neverest/`, the store every account naming no
/// `store.root` shares.
pub fn default_root() -> Result<PathBuf> {
    Ok(state_base()?.join("neverest"))
}

/// The account's store directory: `store.root`, else [`default_root`].
pub fn store_dir(config: &AccountConfig) -> Result<PathBuf> {
    match &config.store.root {
        Some(root) => Ok(root.clone()),
        None => default_root(),
    }
}

/// The stores the selected accounts use, each with its accounts in name
/// order: every account validated, no store a 0.x layout.
pub(crate) fn group_stores(
    selected: Vec<(String, AccountConfig)>,
) -> Result<BTreeMap<PathBuf, Vec<(String, AccountConfig)>>> {
    let mut stores: BTreeMap<PathBuf, Vec<(String, AccountConfig)>> = BTreeMap::new();
    for (name, account_config) in selected {
        account_config
            .validate()
            .with_context(|| format!("Validate account {name}"))?;
        let dir = store_dir(&account_config)?;
        stores.entry(dir).or_default().push((name, account_config));
    }
    for dir in stores.keys() {
        refuse_former_stores(dir)?;
    }
    Ok(stores)
}

/// Why account `name` cannot run in a store `init` never created.
pub(crate) fn not_initialized(name: &str) -> anyhow::Error {
    anyhow!(
        "Account {name} not initialized, run `neverest init -a {name}` or \
         `neverest sync --init -a {name}` first"
    )
}

/// Refuses the neverest directory an earlier neverest kept one store per
/// account in, before anything is created there.
///
/// Only while it holds no store of its own: a former store is synced by
/// the neverest that wrote it, then moved out, which leaves the directory
/// to the shared store; one left inside after that is not looked at.
pub fn refuse_former_stores(dir: &Path) -> Result<()> {
    if dir.join("pimdir.db").exists() || *dir != default_root()? {
        return Ok(());
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };

    let mut former: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("pimdir.db").is_file())
        .collect();
    if former.is_empty() {
        return Ok(());
    }
    former.sort();

    let named: Vec<String> = former
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    Err(layout_error(format!(
        "{} holds the stores an earlier neverest kept per account ({}), and this neverest \
         keeps every account in one store there: sync each with the neverest that wrote it \
         until nothing is queued, move it out of {}, then run `neverest init` again",
        dir.display(),
        named.join(", "),
        dir.display(),
    )))
}

/// Refuses, writing nothing, a store no account of this neverest can run over.
///
/// That is one laid out as an earlier neverest kept it, or at another format
/// version; one not created yet passes. A reader takes no lock and writes
/// nothing, so a dry run checks the store as a real run does.
pub fn check_store(dir: &Path) -> Result<()> {
    refuse_former_stores(dir)?;

    if !dir.join("pimdir.db").exists() {
        return Ok(());
    }

    if let Err(err) = PimdirReader::open(dir) {
        return Err(store_refusal(dir, err));
    }

    StoreState::load(dir)?;
    Ok(())
}

/// Creates the store at `dir`, its user's alone (pimdir STORAGE §18).
///
/// Its sidecar holds no account and no send in flight, so a mark a moved store
/// left names nothing here; it is written with the database, so a database
/// without one only ever describes a store an earlier neverest wrote.
pub fn create_store(dir: &Path) -> Result<()> {
    PimdirStore::open(dir)
        .map_err(|err| store_refusal(dir, err))
        .with_context(|| format!("Create pimdir store {}", dir.display()))?;
    StoreState::default()
        .save(dir)
        .with_context(|| format!("Stamp the store state of {}", dir.display()))?;
    submit::InFlight::new(dir)
        .end()
        .with_context(|| format!("Clear the in-flight send of {}", dir.display()))
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

/// Opens one source handle over the account's store.
///
/// Every collection it writes is grouped under `account` (pimdir STORAGE
/// §9.2), so a store shared by several accounts tells whose is whose.
pub(crate) fn open_store(dir: &Path, source: &str, account: &str) -> Result<PimdirSourceStore> {
    match PimdirStore::open(dir) {
        Ok(store) => Ok(store.for_account(account).for_source(source)),
        Err(err @ PimdirError::Version { .. }) => Err(store_refusal(dir, err)),
        Err(err) => Err(pimdir_error(err).context(format!("Open {source} store"))),
    }
}

/// A store refused for what it is, with its remedy.
///
/// A store opens under its own format version, never migrated (pimdir STORAGE
/// §6): one an earlier neverest wrote is synced by it then moved away, which
/// keeps its queued writes; one a newer neverest wrote calls for an upgrade.
fn store_refusal(dir: &Path, err: PimdirError) -> anyhow::Error {
    match err {
        err @ PimdirError::Version { .. } if written_earlier(&err) => {
            pimdir_error(err).context(format!(
                "The store at {} was written by an earlier neverest and cannot be migrated: \
                 sync it with the neverest that wrote it until nothing is queued, move the \
                 store directory away, then rebuild it with `neverest init` and \
                 `neverest sync`",
                dir.display(),
            ))
        }
        err @ PimdirError::Version { .. } => pimdir_error(err).context(format!(
            "The store at {} was written by a newer neverest; upgrade neverest to sync it",
            dir.display(),
        )),
        err => pimdir_error(err).context(format!("Open the store at {}", dir.display())),
    }
}

/// Whether a store refused for its format version was written by an earlier
/// draft than this neverest's.
///
/// That is one naming no version (every draft before storage-draft-08) or a
/// draft below [`FORMAT_VERSION`](io_pimdir::sql::FORMAT_VERSION); a later
/// draft or a release is a newer neverest's.
fn written_earlier(err: &PimdirError) -> bool {
    let PimdirError::Version { found } = err else {
        return false;
    };
    let Some(found) = found else {
        return true;
    };
    let draft = |version: &str| {
        version
            .strip_prefix("storage-draft-")
            .and_then(|number| number.parse::<u32>().ok())
    };
    match (draft(found), draft(io_pimdir::sql::FORMAT_VERSION)) {
        (Some(found), Some(ours)) => found < ours,
        _ => false,
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
    /// Whether the run confirmed the absences its listings find.
    ///
    /// A listing leaving most of a collection out then drops it rather than
    /// parks (pimdir SYNC §5, §8).
    accept_absences: bool,
    /// Whether the configuration caps what the source lists of a collection.
    ///
    /// Every page of its rounds then states the cap (pimdir SYNC §4, §5).
    capped: bool,
    /// What its syncs reported beyond their events, drained by
    /// [`itemize_noted`].
    noted: Noted,
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

    /// The options the side's sync runs under, pushing when `push`.
    fn sync_options(&self, push: bool) -> PimdirSyncOptions {
        PimdirSyncOptions {
            push,
            rights: self.push_rights(),
            conflict: self.conflict_policy(),
            scope: self.scope.clone(),
            accept_absences: self.accept_absences,
            ..Default::default()
        }
    }

    /// How an upgrade on the side settles a divergence whose body it fetches.
    ///
    /// By the policy of the side's pushing sync (pimdir SYNC §6), a dry run or
    /// a side that writes nothing back pushing nothing.
    fn upgrade_options(&self) -> PimdirUpgradeOptions {
        PimdirUpgradeOptions::from(&self.sync_options(!self.dry_run && self.writable()))
    }
}

/// Runs the whole account sync and returns the report.
///
/// Sources sync against the store, or each target ([`AccountConfig::mode`]).
/// `declare_only` records collections and moves no item, `intents_only` does
/// the queued intents only; a `drained` of `None` declares and drains here.
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
    accept_absences: bool,
    pacing: Pacing,
    declare_only: bool,
    intents_only: bool,
    progress: Progress<'_>,
    drained: Option<SyncOutput>,
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

    let real_dir = store_dir(account_config)?;
    let dry_replica = dry_run.then(|| DryRunReplica::new(&real_dir)).transpose()?;
    let work_dir = match &dry_replica {
        Some(replica) => replica.dir.clone(),
        None => real_dir.clone(),
    };
    fs::create_dir_all(&work_dir)
        .with_context(|| format!("Create replica dir {} error", work_dir.display()))?;

    let mut sidecar = StoreState::load(&work_dir)?;
    let mut state = sidecar.account(&account_name)?;

    let mut report = SyncOutput {
        account: account_name.clone(),
        dry_run,
        ..Default::default()
    };

    state.check_mode(&mode, accept_mode)?;

    match drained {
        Some(drained) => report.absorb(drained),
        None => {
            if !dry_run {
                declare(&work_dir, &account_name, account_config, &mode, &state)?;
            }

            if !declare_only {
                let first = running
                    .first()
                    .expect("a validated account has at least one source");
                let mut store = open_store(&work_dir, first, &account_name)?;
                match drain_store(&mut store) {
                    Ok(mut drained) => {
                        report.absorb(drained.remove(&account_name).unwrap_or_default());
                    }
                    Err(err) => {
                        warn!("drain failed, actions stay queued: {err:#}");
                        let failure = StepFailure::new(FailedStep::Queue, None, &err);
                        report.failures.push(failure);
                    }
                }
            }
        }
    }

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
                accept_absences,
                connections,
                pacing,
                declare_only,
                progress,
                &mut state,
                &mut report,
            )
        } else {
            let run = AccountRun {
                name: &account_name,
                config: account_config,
                account: &account,
                mode: &mode,
                scope,
                work_dir: &work_dir,
                dry_run,
                accept_absences,
                connections,
                progress,
            };
            run_targets(
                run,
                &endpoints,
                source_name,
                source,
                collection_filter.clone(),
                &mut state,
                &mut report,
            )
        };

        if let Err(err) = outcome {
            note_source_failure(source_name, err, &mut report);
        }
    }

    for (source, until) in account.throttled() {
        report.throttled.push(ThrottledSource {
            source,
            until: throttle::rfc3339(until),
        });
    }
    for (source, bytes) in account.downloaded() {
        report.downloaded.push(SourceDownload { source, bytes });
    }

    match PimdirStore::open(&work_dir) {
        Ok(store) => {
            report_parked(&store, &account_name, &mut report);
            count_conflicts(&store, &account_name, &mut report);
        }
        Err(err) => {
            warn!("cannot open the store to read what it left behind: {err}");
            let err = pimdir_error(err).context("Open the store to count what waits");
            report
                .failures
                .push(StepFailure::new(FailedStep::Conflicts, None, &err));
        }
    }

    warn_conflicts(&report);

    if !dry_run {
        state.record_mode(&mode, accept_mode);

        if let Err(err) = declare(&work_dir, &account_name, account_config, &mode, &state) {
            warn!("cannot declare what the sources hold after the run: {err:#}");
        }

        if !no_purge && !intents_only {
            let sweeper = running
                .first()
                .expect("a validated account has at least one source");
            let mut store = open_store(&work_dir, sweeper, &account_name)?;
            sweep_retained(account_config, &mut store, &mut report);
        }

        #[cfg(feature = "search")]
        let quick = intents_only || declare_only;
        #[cfg(feature = "search")]
        crate::sync::index::after_run(&work_dir, &account_config.store, quick, &mut report.index);

        sidecar.accounts.insert(account_name, state);
        sidecar.save(&work_dir)?;
    }

    Ok(report)
}

/// Readies one account of a run before the store's drain.
///
/// Refused when `init` never stamped it or its mode would discard what the
/// last run kept (unless `accept_mode`), then declared (pimdir STORAGE §15.6)
/// from the configuration and sidecar alone, reading no credential.
pub fn prepare(dir: &Path, account: &str, config: &AccountConfig, accept_mode: bool) -> Result<()> {
    let mode = config.mode()?;
    let state = StoreState::load(dir)?.account(account)?;
    state.check_mode(&mode, accept_mode)?;
    declare(dir, account, config, &mode, &state)
}

/// Drains the store's queue once, every account's (pimdir STORAGE §15.2).
///
/// What it applied is shared out by account, a row on a collection grouped
/// under no account applied and reported nowhere. A store step failing fails
/// the drain.
pub(crate) fn drain_store(store: &mut PimdirSourceStore) -> Result<BTreeMap<String, SyncOutput>> {
    let mut drained = SyncOutput::default();
    drain_queues_into(store, &mut drained)?;

    let owners = collection_accounts(store);
    let mut by_account: BTreeMap<String, SyncOutput> = BTreeMap::new();
    for entry in drained.drained {
        if let Some(account) = owners.get(&entry.collection) {
            by_account
                .entry(account.clone())
                .or_default()
                .drained
                .push(entry);
        }
    }
    for body in drained.overruled {
        if let Some(account) = owners.get(&body.collection) {
            by_account
                .entry(account.clone())
                .or_default()
                .overruled
                .push(body);
        }
    }

    Ok(by_account)
}

/// The account each collection of the store is grouped under, by id.
pub(crate) fn collection_accounts(store: &PimdirStore) -> HashMap<String, String> {
    match store.list_collections() {
        Ok(collections) => collections
            .into_iter()
            .filter_map(|collection| Some((collection.id, collection.account?)))
            .collect(),
        Err(err) => {
            warn!("cannot list whose collections the store holds: {err}");
            HashMap::new()
        }
    }
}

/// Declares what every source of the account can do (pimdir STORAGE §15.6).
///
/// Replaces earlier declarations with what its backend and rights allow, and
/// `calendar.component.vtodo` `none` source-wide where a provider keeps tasks
/// apart, else on the calendars its last listings (`state`) took no tasks on.
pub(crate) fn declare(
    dir: &Path,
    account: &str,
    config: &AccountConfig,
    mode: &AccountMode,
    state: &AccountState,
) -> Result<()> {
    let sources = config.sources()?;
    let endpoints = config.endpoints()?;
    let targets: Vec<(&str, &SourceConfig)> = mode
        .targets
        .iter()
        .filter_map(|name| Some((name.as_str(), endpoints.get(name)?)))
        .collect();
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
        let namespace = format!("{name}/");
        let mut held = Vec::new();
        let mut fixed = BTreeSet::new();
        let mut drafts = BTreeSet::new();
        for collection in &collections {
            let syncing = store
                .collection_sources(&collection.id)
                .with_context(|| format!("List the sources of {}", collection.id))?;
            if syncing.contains(name) || collection.id.starts_with(&namespace) {
                held.push(collection.id.clone());
                if collection.role.is_some() {
                    fixed.insert(collection.id.clone());
                }
                if collection.role.as_deref() == Some("drafts") {
                    drafts.insert(collection.id.clone());
                }
            }
        }
        let paired = !mode.targets.is_empty();
        let mut capabilities = capability::declaration(source, mode.one_way, paired, &held, &fixed);
        capability::drafts_only(source, &mut capabilities, &drafts);
        match capability::vtodo_wide(source, &targets) {
            Some(wide) => capabilities.push(wide),
            None if capabilities.is_empty() => {}
            None => {
                if let Some(taskless) = state.taskless.get(name) {
                    capabilities.extend(capability::vtodo(&held, taskless));
                }
            }
        }
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
    ///
    /// Dates are one fixed RFC 3339 shape in UTC, so their strings sort as the
    /// instants do, and `None` sorts below any date, hence last.
    fn compare(self, a: &HydrateTarget, b: &HydrateTarget) -> CmpOrdering {
        match self {
            Self::Largest => b.size.cmp(&a.size),
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
    /// Only the bodies of the items this run listed, the others left owed.
    ///
    /// An arrival is then readable at once, never behind an earlier backlog.
    Listed,
}

/// How many newest bodies of each collection `--prime` downloads by default.
///
/// Under `--download-order newest`: what a reader sees of a mailbox before
/// scrolling, the first chunk the Android client shows.
pub const DEFAULT_PRIME: usize = 50;

/// How a run orders and paces its work, never how much of it there is.
///
/// Whatever the pacing, a run that ends has listed every page and downloaded
/// every body its collections owe ([`Bodies::Listed`] aside).
#[derive(Clone, Copy, Debug)]
pub struct Pacing {
    /// The order bodies download in.
    pub order: DownloadOrder,
    /// How many newest bodies each collection downloads before any lists a
    /// second page; 0 lists each collection whole in turn.
    pub prime: usize,
    /// Which owed bodies the run downloads.
    pub bodies: Bodies,
    /// How long a source waits on a throttling provider before giving up.
    pub max_wait: std::time::Duration,
}

impl Pacing {
    /// The pacing of a run, `prime` defaulting by the order.
    ///
    /// On under `newest`, the order a caller picks to show mail early; off
    /// under `largest`, the fastest whole run, which priming only slows.
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

#[cfg(test)]
mod tests;

#[cfg(all(test, feature = "msgraph"))]
mod graph_budget;

#[cfg(all(test, feature = "gmail"))]
mod gmail_bodies;

#[cfg(all(test, feature = "gmail"))]
mod gmail_heads;

#[cfg(all(test, feature = "msgraph"))]
mod graph_heads;

#[cfg(all(test, feature = "msgraph"))]
mod graph_one_table;

#[cfg(test)]
mod collection_intent_tests;
