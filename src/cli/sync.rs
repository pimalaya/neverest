//! # Sync command
//!
//! Runs the io-pimdir reconcile over every account the run covers, store by
//! store under each store's lock, and prints the resulting
//! [`crate::sync::report::SyncRunOutput`].

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, TryLockError},
    path::{Path, PathBuf},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Error, Result, anyhow, bail};
use chrono::Utc;
use clap::{ArgAction, Parser};
use io_pimdir::{
    client::{PimdirStore, reader::PimdirCollection},
    object::PimdirHash,
    placement::{PimdirBase, PimdirFlags},
};
use log::{debug, info, warn};
use pimalaya_cli::{error::code_of, printer::Printer};

use crate::{
    cli::exit::Exit,
    config::{AccountConfig, CollectionFilter, Config},
    error::pimdir_error,
    offline::{
        driver::{self, Bodies, DownloadOrder, Pacing},
        local,
        progress::Progress,
        state::StoreState,
    },
    sync::{
        event::SyncEvent,
        report::{AccountSync, SyncOutput, SyncRunOutput},
    },
};

/// How long a run waits for another run's store lock before giving up.
///
/// Long enough for a connector-triggered scoped run to queue behind a cron
/// tick, short enough to fail loudly on a wedged holder.
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(60);

/// How often the waiter retries the lock.
const LOCK_POLL: Duration = Duration::from_millis(500);

/// The file of the store's local/ a run holds its advisory lock on.
const SYNC_LOCK: &str = "sync.lock";

/// Synchronizes the collections and items of every account, or of those `-a`
/// names, through the pimdir store they share.
///
/// Each source is reconciled against the store, then against every target the
/// account names. Sources never meet: an item one holds crosses to the
/// targets, never to another source, so a mail source and a contacts source
/// under one account stay apart. Accounts run one after the other, the store
/// locked once for the whole run; one that fails is reported and the next
/// one runs.
///
/// The three filter flags keep their pre-`generic-pim-sync` spellings
/// (`--include-mailbox`, `--exclude-mailbox`, `--all-mailboxes`) as hidden
/// aliases, so existing scripts keep working.
///
/// Exit code 0 means the run reconciled everything and left nothing waiting,
/// 1 that it failed, and 2 that it reconciled its collections and left
/// something waiting: a parked conflict, a duplicate `UID` the other side
/// refuses, a write it would not take, a send or an intent parked (refused
/// for good, or possibly delivered and never sent again on its own), a
/// queued action the store holds parked, a listing that left out most of a
/// collection (see `--accept-absences`) or a change withheld over a body the
/// store cannot serve. None is a failure: each is one item
/// wide and halts nothing, and under a supervisor restarting on failure they
/// would loop over a state no supervisor can fix. Exit code 3 means the run
/// could not do all its work and a rerun picks it up: a source it could not
/// reach (offline) or an SMTP channel it could not open, a hunk, a send or an
/// intent that failed without parking, a collection still moving when the
/// run stopped, an account that failed, or a store another run held past
/// the wait. It wins over 2.
#[derive(Debug, Parser)]
pub struct SyncCommand {
    /// Print the patch that would be applied, without applying it.
    #[arg(long, short = 'd')]
    pub dry_run: bool,
    /// Synchronize only the given collection names (repeatable, ASCII
    /// case-insensitive).
    #[arg(long, short = 'm', alias = "include-mailbox")]
    #[arg(value_name = "COLLECTION", action = ArgAction::Append)]
    #[arg(
        conflicts_with = "exclude_collection",
        conflicts_with = "all_collections"
    )]
    pub include_collection: Vec<String>,
    /// Skip the given collection names (repeatable, ASCII case-insensitive).
    #[arg(long, short = 'x', alias = "exclude-mailbox")]
    #[arg(value_name = "COLLECTION", action = ArgAction::Append)]
    #[arg(
        conflicts_with = "include_collection",
        conflicts_with = "all_collections"
    )]
    pub exclude_collection: Vec<String>,
    /// Synchronize every collection on both sides, ignoring config filters.
    #[arg(long, short = 'A', alias = "all-mailboxes")]
    #[arg(
        conflicts_with = "include_collection",
        conflicts_with = "exclude_collection"
    )]
    pub all_collections: bool,
    /// Drop the account's collections before running, then resync them
    /// whole, every other account of the store left as it was.
    ///
    /// Needs exactly one account. Refused while its collections hold work no
    /// server has, which the refusal lists: a queued or parked action, which
    /// a drain applies or its frontend cancels first, and conflicts,
    /// unpushed changes and retained items, which `--force` drops.
    #[arg(long, conflicts_with_all = [
        "dry_run",
        "include_collection",
        "exclude_collection",
        "source",
    ])]
    pub reset: bool,
    /// With `--reset`, drop the account's conflicts, unpushed changes and
    /// retained items with its collections. A queued or parked action still
    /// refuses the reset.
    #[arg(long, requires = "reset")]
    pub force: bool,
    /// Max connections per side for concurrent body fetches (default 4, or the
    /// account's `connections`). Keep it under your provider's per-account cap.
    #[arg(long, short = 'j', value_name = "N")]
    pub connections: Option<usize>,
    /// Skip the retention sweep: keep every retained (soft-deleted) item,
    /// whatever `store.purge-after` says.
    #[arg(long)]
    pub no_purge: bool,
    /// Synchronize only the given sources (repeatable).
    #[arg(long, short = 's', value_name = "SOURCE", action = ArgAction::Append)]
    pub source: Vec<String>,
    /// Accept a mode change that discards data, and remember the answer.
    ///
    /// Turning `one-way` on makes the sources authoritative, so the next run
    /// discards whatever the other side changed on its own rather than
    /// merging it. That first run is the one that loses them, which is why it
    /// is refused until this says otherwise.
    #[arg(long)]
    pub accept_mode: bool,
    /// Drop the items a listing left out, even when it left out most of a
    /// collection, for this run.
    ///
    /// A listing leaving out more than half of at least 20 items, or naming
    /// none while the collection held some, is read as a truncated or
    /// emptied answer: nothing is dropped, the run reports it and exits 2.
    /// Once the server is known to hold no more, this confirms the
    /// absences, also for the rebuild of a renumbered IMAP mailbox.
    #[arg(long)]
    pub accept_absences: bool,
    /// The order bodies download in: `largest` first, the fastest whole run,
    /// or `newest` first, so recent mail is readable early on a large account.
    #[arg(long, value_enum, value_name = "ORDER", default_value_t)]
    pub download_order: DownloadOrder,
    /// List every collection's newest page and download its N newest bodies
    /// before any collection lists its second page, the rest following
    /// as usual: every collection is readable early, whatever its size.
    /// Orders the run, never shortens it. 0 turns it off; the default is
    /// 50 under `--download-order newest`, 0 under `largest`.
    #[arg(long, value_name = "N")]
    pub prime: Option<usize>,
    /// Which owed bodies the run downloads: `all` of them, or only those of
    /// the items this run `listed`, leaving the rest for a later run, so a
    /// new mail is readable at once rather than behind an older backlog.
    #[arg(long, value_enum, value_name = "WHICH", default_value_t)]
    pub bodies: Bodies,
    /// The longest a source waits at once on a provider throttling it, in
    /// seconds. A provider asking for more makes the source give up for
    /// the run, which is then incomplete (exit 3).
    #[arg(long, value_name = "SECONDS", default_value_t = 120)]
    pub max_wait: u64,
    /// List every collection the sources hold and record its kind, name
    /// and role in the store, whatever the collection filter, syncing no
    /// item. What a client reads to offer the collections it could sync,
    /// and to learn which one is the inbox or the default calendar. Only
    /// the queued collection intents run (create, rename, delete); sends and
    /// the other intents wait.
    #[arg(long, conflicts_with_all = ["reset", "download_order", "prime", "bodies"])]
    pub declare_only: bool,
    /// Perform what frontends queued and nothing else: apply the queue,
    /// then send the queued messages and perform the queued collection
    /// creations, invitation replies and cancellations, listing no
    /// collection and moving no other item. An endpoint opens only when an
    /// intent needs it, so a message leaving through an `smtp` channel
    /// opens that channel alone. What a frontend runs right after queueing
    /// a send; the copy a send files in Sent is pushed by the next sync.
    #[arg(long, conflicts_with_all = [
        "dry_run",
        "reset",
        "declare_only",
        "include_collection",
        "exclude_collection",
        "all_collections",
        "since",
        "download_order",
        "prime",
        "bodies",
    ])]
    pub intents_only: bool,
    /// Sync mail dated since then only, for this run: a duration back from
    /// today (`30d`, `12w`, `6mo`, `1y`), a date (`2026-01-01`) or an RFC 3339
    /// instant. Overrides the account's `item.filter.since`; refused on an
    /// account syncing contacts or calendars. Mail older than it is neither
    /// listed nor deleted.
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,
}

impl SyncCommand {
    /// Reconciles every account the run covers, or plans them under
    /// `--dry-run`.
    pub fn execute(
        self,
        printer: &mut (impl Printer + Send),
        config_paths: &[PathBuf],
        accounts: &[String],
    ) -> Result<Exit> {
        let config = Config::load_or_wizard(printer, config_paths)?;
        let selected = match self.reset {
            true => vec![config.select_one(accounts, "A reset")?],
            false => config.select(accounts)?,
        };

        // NOTE: the configuration is one document, so an account it does
        // not validate fails the run before anything does.
        let mut stores: BTreeMap<PathBuf, Vec<(String, AccountConfig)>> = BTreeMap::new();
        for (name, account_config) in selected {
            account_config
                .validate()
                .with_context(|| format!("Validate account {name}"))?;
            let dir = driver::store_dir(&account_config)?;
            stores.entry(dir).or_default().push((name, account_config));
        }
        for dir in stores.keys() {
            driver::refuse_former_stores(dir)?;
        }

        let order: Vec<String> = stores
            .values()
            .flatten()
            .map(|(name, _)| name.clone())
            .collect();
        let mut outcomes: Vec<AccountSync> = Vec::new();

        // NOTE: every store's lock before any account runs, in path order, so
        // two runs over several stores never wait on each other crosswise.
        let mut locks = Vec::new();
        let mut runnable = Vec::new();
        for (dir, accounts) in stores {
            if !dir.join("pimdir.db").exists() {
                for (name, _) in accounts {
                    let err = anyhow!(
                        "Account {name} not initialized, run `neverest init -a {name}` first"
                    );
                    outcomes.push(failed(name, err));
                }
                continue;
            }

            let lock = wait_store_lock(&dir, LOCK_TIMEOUT)
                .with_context(|| format!("Acquire the lock of the store at {}", dir.display()))?;
            match lock {
                Some(lock) => {
                    locks.push(lock);
                    driver::check_store(&dir)?;
                    runnable.push((dir, accounts));
                }
                None => {
                    for (name, _) in accounts {
                        outcomes.push(busy(name));
                    }
                }
            }
        }

        if self.reset
            && let Some((dir, accounts)) = runnable.first()
        {
            let (name, account_config) = &accounts[0];
            let namespaced = account_config.namespaced(name)?;
            reset_account(dir, name, &namespaced, self.force)
                .with_context(|| format!("Reset account {name}"))?;
        }

        // NOTE: the driver reports from its worker threads, which share the
        // printer. A line that cannot be printed stops no sync.
        let printer = Mutex::new(printer);
        let json = printer.lock().unwrap().is_json();
        let total = order.len();

        for (dir, accounts) in runnable {
            for (name, account_config, drained) in self.ready(&dir, accounts, &mut outcomes)? {
                let sink = |mut event: SyncEvent| {
                    event.name_endpoints(&name);
                    if let Err(err) = printer.lock().unwrap().event(event) {
                        debug!("cannot print sync event: {err:#}");
                    }
                };
                let progress = match json {
                    true => Progress::events(&sink),
                    false => Progress::default(),
                };

                let position = order.iter().position(|n| *n == name).unwrap_or(0) + 1;
                progress.emit(|| SyncEvent::AccountStarted {
                    account: name.clone(),
                    position,
                    total,
                });

                let outcome = self.run_account(&name, &account_config, drained, progress);
                outcomes.push(match outcome {
                    Ok(report) => ran(name, report),
                    Err(err) => {
                        warn!("account {name} sync error: {err:#}");
                        failed(name, err)
                    }
                });
            }
        }
        drop(locks);

        outcomes.sort_by_key(|outcome| order.iter().position(|name| *name == outcome.account));
        let output = SyncRunOutput { accounts: outcomes };

        // NOTE: read before printing, which consumes the output.
        let exit = Exit::from(&output);
        printer.into_inner().unwrap().out(output)?;

        Ok(exit)
    }

    /// The accounts of the store at `dir` ready to run, each with what the
    /// store's drain applied on its collections, `None` when the run drains
    /// by itself (a dry run, on its replica). One that cannot be readied
    /// lands in `outcomes`.
    ///
    /// The drain is the store's: it runs once, after every account was
    /// readied, so it applies each account's queue under the capabilities
    /// the account just declared (pimdir STORAGE §15.6).
    fn ready(
        &self,
        dir: &Path,
        accounts: Vec<(String, AccountConfig)>,
        outcomes: &mut Vec<AccountSync>,
    ) -> Result<Vec<(String, AccountConfig, Option<SyncOutput>)>> {
        if self.dry_run {
            let ready = accounts
                .into_iter()
                .map(|(name, config)| (name, config, None));
            return Ok(ready.collect());
        }

        let mut ready = Vec::new();
        for (name, account_config) in accounts {
            let prepared = account_config
                .namespaced(&name)
                .and_then(|namespaced| driver::prepare(dir, &name, &namespaced, self.accept_mode));
            match prepared {
                Ok(()) => ready.push((name, account_config)),
                Err(err) => outcomes.push(failed(name, err)),
            }
        }

        let mut drained = BTreeMap::new();
        if !self.declare_only
            && let Some((name, account_config)) = ready.first()
        {
            let namespaced = account_config.namespaced(name)?;
            let mode = namespaced.mode()?;
            let first = mode
                .sources
                .first()
                .expect("a validated account has at least one source");
            let mut store = driver::open_store(dir, first, name)?;
            drained = driver::drain_store(&mut store);
        }

        Ok(ready
            .into_iter()
            .map(|(name, config)| {
                let mine = drained.remove(&name).unwrap_or_default();
                (name, config, Some(mine))
            })
            .collect())
    }

    /// Runs one account and names its endpoints as its configuration does.
    fn run_account(
        &self,
        name: &str,
        account_config: &AccountConfig,
        drained: Option<SyncOutput>,
        progress: Progress<'_>,
    ) -> Result<SyncOutput> {
        let mode = account_config.mode()?;
        if let Some(unknown) = self.source.iter().find(|s| !mode.sources.contains(s)) {
            bail!(
                "Account {name} has no source named {unknown}, it declares {}",
                mode.sources.join(", "),
            );
        }
        let only: Vec<String> = self.source.iter().map(|s| format!("{name}/{s}")).collect();

        let scope = account_config.scope(self.since.as_deref(), &self.source, Utc::now())?;

        let cli_filter = if !self.include_collection.is_empty() {
            Some(CollectionFilter::Include(self.include_collection.clone()))
        } else if !self.exclude_collection.is_empty() {
            Some(CollectionFilter::Exclude(self.exclude_collection.clone()))
        } else if self.all_collections {
            Some(CollectionFilter::All)
        } else {
            None
        };

        let connections = self
            .connections
            .or(account_config.connections)
            .unwrap_or(4)
            .max(1);

        let mut report = driver::run(
            name,
            &account_config.namespaced(name)?,
            cli_filter,
            &scope,
            self.dry_run,
            connections,
            self.no_purge,
            &only,
            self.accept_mode,
            self.accept_absences,
            Pacing::new(
                self.download_order,
                self.prime,
                self.bodies,
                Duration::from_secs(self.max_wait),
            ),
            self.declare_only,
            self.intents_only,
            progress,
            drained,
        )?;

        report.name_endpoints();
        Ok(report)
    }
}

/// Takes the store's advisory sync.lock for the whole run.
///
/// Waits up to `timeout` so cron ticks and connector-triggered scoped runs
/// serialize instead of failing, then errors out. The kernel releases the
/// lock on FD close, so there is no PID file to clean up.
pub fn acquire_store_lock(store_dir: &Path, timeout: Duration) -> Result<File> {
    match wait_store_lock(store_dir, timeout)? {
        Some(file) => Ok(file),
        None => bail!(
            "Another sync still holds {} after {}s",
            local::path(store_dir).join(SYNC_LOCK).display(),
            timeout.as_secs()
        ),
    }
}

/// Takes the store's sync.lock, waiting up to `timeout`: `None` while
/// another run still holds it then.
pub fn wait_store_lock(store_dir: &Path, timeout: Duration) -> Result<Option<File>> {
    let (lock_path, file) = open_store_lock(store_dir)?;

    let deadline = Instant::now() + timeout;
    let mut waiting = false;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                if !waiting {
                    debug!("store is locked by another run, waiting");
                    waiting = true;
                }
                thread::sleep(LOCK_POLL);
            }
            Err(TryLockError::Error(err)) => {
                return Err(err)
                    .with_context(|| format!("Acquire sync lock {} error", lock_path.display()));
            }
        }
    }
}

/// Takes the store's sync.lock without waiting: `None` while another run
/// holds it.
pub fn try_store_lock(store_dir: &Path) -> Result<Option<File>> {
    let (lock_path, file) = open_store_lock(store_dir)?;

    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(err)) => {
            Err(err).with_context(|| format!("Acquire sync lock {} error", lock_path.display()))
        }
    }
}

/// Opens, creating it if needed, the file the sync.lock is taken on: a
/// file of the store's local/, neverest's own state there (pimdir STORAGE
/// §3), which is created with it.
fn open_store_lock(store_dir: &Path) -> Result<(PathBuf, File)> {
    let local = local::ensure(store_dir)
        .with_context(|| format!("Create {}", local::path(store_dir).display()))?;
    let lock_path = local.join(SYNC_LOCK);
    let file = local::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("Open sync lock {} error", lock_path.display()))?;
    Ok((lock_path, file))
}

/// The outcome of an account that ran.
fn ran(account: String, report: SyncOutput) -> AccountSync {
    AccountSync {
        account,
        busy: false,
        code: None,
        error: None,
        report: Some(report),
    }
}

/// The outcome of an account that failed, keeping the failure's code.
fn failed(account: String, err: Error) -> AccountSync {
    AccountSync {
        account,
        busy: false,
        code: code_of(&err),
        error: Some(format!("{err:#}")),
        report: None,
    }
}

/// The outcome of an account whose store another run held.
fn busy(account: String) -> AccountSync {
    AccountSync {
        account,
        busy: true,
        code: None,
        error: None,
        report: None,
    }
}

/// Starts `account` over inside the store it shares: drops its collections
/// with what the store records under them (items, bindings, checkpoints,
/// coverage, capability rows), its sources' source-wide capability rows and
/// its sidecar entry, then stamps a fresh entry. Every other account of the
/// store is left as it was.
///
/// `config` is the account as the store sees it
/// ([`AccountConfig::namespaced`]). Refused while a collection of the
/// account holds a queued or parked action, which the store refuses to drop
/// with its collection (pimdir GUIDE "Removal"), naming each row and the way
/// out; without `force`, also while they hold other work no server has,
/// naming it and `--force`.
fn reset_account(dir: &Path, account: &str, config: &AccountConfig, force: bool) -> Result<()> {
    let mode = config.mode()?;
    StoreState::load(dir)?.account(account)?;

    let mut store = PimdirStore::open(dir)
        .map_err(pimdir_error)
        .with_context(|| format!("Open the store at {}", dir.display()))?
        .for_account(account);
    let collections = store
        .list_collections_by_account(Some(account))
        .context("List the collections of the account")?;
    let ids: BTreeSet<&str> = collections.iter().map(|c| c.id.as_str()).collect();

    let mut actions = Vec::new();
    for row in store.list_pending_actions()? {
        if ids.contains(row.collection.as_str()) {
            let kind = row.action.kind();
            actions.push(format!(
                "- queued action #{} ({kind}) on {}",
                row.id, row.collection
            ));
        }
    }
    for row in store.parked_actions()? {
        if ids.contains(row.collection.as_str()) {
            actions.push(format!(
                "- parked action #{} ({}) on {}: {}",
                row.id, row.action, row.collection, row.error
            ));
        }
    }
    let work = match force {
        true => Vec::new(),
        false => pending_work(&store, &collections)?,
    };

    if !actions.is_empty() || !work.is_empty() {
        let mut remedies = Vec::new();
        if !actions.is_empty() {
            remedies.push(format!(
                "`neverest drain -a {account}` or a sync applies a queued action, and the \
                 frontend that queued a parked one cancels it (`himalaya pimdir queue cancel \
                 <ROW>`)"
            ));
        }
        if !work.is_empty() {
            remedies.push(format!(
                "`neverest sync --reset --force -a {account}` drops the rest with the \
                 account's collections"
            ));
        }
        let listed: Vec<String> = actions.into_iter().chain(work).collect();
        bail!(
            "Cannot reset account {account} while its collections hold work no server has:\n\
             {}\n{}",
            listed.join("\n"),
            remedies.join("; "),
        );
    }

    for collection in &collections {
        store
            .delete_collection(&collection.id)
            .map_err(pimdir_error)
            .with_context(|| format!("Drop collection {}", collection.id))?;
    }
    for source in config.endpoints()?.keys() {
        store
            .declare(source, &[])
            .map_err(pimdir_error)
            .with_context(|| format!("Drop the capabilities of source {source}"))?;
    }
    // NOTE: the bodies only the dropped collections held, whichever account
    // referenced them last; a failure leaves them to the next collection.
    if let Err(err) = store.collect_garbage() {
        warn!("cannot collect what the reset of account {account} released: {err}");
    }
    drop(store);

    StoreState::stamp(dir, account, &mode)?;
    info!(
        "reset: dropped {} collection(s) of account {account}",
        collections.len()
    );
    Ok(())
}

/// What `collections` hold that no server has, one line per collection: its
/// conflicts, its retained items (bound to no server), its pending deletes
/// and the items whose state no binding's base holds.
fn pending_work(store: &PimdirStore, collections: &[PimdirCollection]) -> Result<Vec<String>> {
    let mut pending = Vec::new();

    for collection in collections {
        let hub = store.load_hub(&collection.id)?;
        let (mut conflicts, mut retained, mut deletes, mut unpushed) = (0, 0, 0, 0);

        for item in hub.items.values() {
            // NOTE: a delivered create is based under its provisional
            // handle, on a base naming nothing (pimdir SYNC §2): it holds
            // nothing a listing confirmed, so it is a staged create still.
            let held = item.sources.values().any(|binding| {
                !binding.handle.is_provisional()
                    && binding
                        .base
                        .as_ref()
                        .is_some_and(|base| holds(base, &item.flags, &item.object))
            });
            if item.conflicted || item.sources.values().any(|binding| binding.conflicted) {
                conflicts += 1;
            } else if item.sources.is_empty() {
                retained += 1;
            } else if item.deleted {
                deletes += 1;
            } else if !held {
                unpushed += 1;
            }
        }

        let counts = [
            (conflicts, "conflict(s)"),
            (retained, "retained item(s)"),
            (deletes, "pending delete(s)"),
            (unpushed, "unpushed change(s)"),
        ];
        let named: Vec<String> = counts
            .into_iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, what)| format!("{count} {what}"))
            .collect();
        if !named.is_empty() {
            pending.push(format!("- {}: {}", collection.id, named.join(", ")));
        }
    }

    Ok(pending)
}

/// Whether a binding's base holds an item's flags and body, what a server
/// was last known to hold. Flags or a body read on one side only compare as
/// held.
fn holds(base: &PimdirBase, flags: &PimdirFlags, object: &Option<PimdirHash>) -> bool {
    let flags = match (base.flags.known(), flags.known()) {
        (Some(base), Some(item)) => base == item,
        _ => true,
    };
    let object = match (&base.object, object) {
        (Some(base), Some(item)) => base == item,
        _ => true,
    };
    flags && object
}

#[cfg(test)]
mod tests {
    use std::sync::PoisonError;

    use io_pimdir::{
        change::PimdirWriteOp,
        client::{producer::PimdirProducer, reader::PimdirReader},
        codec::PimdirAction,
        collection::PimdirCollectionId,
        object::PimdirObject,
        placement::{
            PimdirHandle, PimdirLevel, PimdirLinkId, PimdirPlacement, PimdirSortKey, PimdirStatus,
        },
        summary,
    };

    use crate::{cli::sync::*, conflict::merger::SPAWNS};

    /// Ordering and pacing flags mean nothing to a run that lists nothing,
    /// so they are refused beside `--intents-only` and `--declare-only`;
    /// `--max-wait` still bounds the sends an intents run makes.
    #[test]
    fn pacing_flags_are_refused_where_nothing_is_listed() {
        let parse = |args: &[&str]| {
            SyncCommand::try_parse_from(std::iter::once("sync").chain(args.iter().copied()))
        };
        for only in ["--intents-only", "--declare-only"] {
            for flag in [
                &["--prime", "5"][..],
                &["--bodies", "listed"],
                &["--download-order", "newest"],
            ] {
                let mut args = vec![only];
                args.extend_from_slice(flag);
                assert!(parse(&args).is_err(), "{args:?} is refused");
            }
            assert!(parse(&[only]).is_ok());
        }
        assert!(parse(&["--intents-only", "--max-wait", "10"]).is_ok());
        let sync = parse(&["--prime", "0", "--bodies", "listed", "--max-wait", "30"]).unwrap();
        assert_eq!(sync.prime, Some(0));
        assert_eq!(sync.bodies, Bodies::Listed);
        assert_eq!(sync.max_wait, 30);
    }

    /// A second run waits for the lock up to its timeout, then answers
    /// none, which the run reports as the store busy.
    #[test]
    fn a_second_run_waits_out_the_lock_then_gives_up() {
        let dir = tempfile::tempdir().unwrap();

        let held = acquire_store_lock(dir.path(), Duration::from_millis(1)).unwrap();

        let start = Instant::now();
        let waited = wait_store_lock(dir.path(), Duration::from_millis(50)).unwrap();
        assert!(waited.is_none());
        assert!(start.elapsed() >= Duration::from_millis(50));
        let err = acquire_store_lock(dir.path(), Duration::from_millis(1)).unwrap_err();
        assert!(format!("{err:#}").contains("Another sync still holds"));

        drop(held);
        acquire_store_lock(dir.path(), Duration::from_millis(1)).unwrap();
    }

    /// The lock is neverest's own state: a file of the store's local/, the
    /// one place pimdir leaves an owner in a store directory (pimdir
    /// STORAGE §3), and nothing of it beside the store's files.
    #[test]
    fn the_sync_lock_is_kept_under_local() {
        let dir = tempfile::tempdir().unwrap();

        let _held = acquire_store_lock(dir.path(), Duration::from_millis(1)).unwrap();

        assert!(dir.path().join("local").join("sync.lock").is_file());
        assert!(!dir.path().join("sync.lock").exists());
    }

    /// A reset is the whole account's: beside a dry run it would drop the
    /// real collections before planning, and beside a filter it would
    /// reset more than it names. `--force` means nothing without it.
    #[test]
    fn a_reset_is_refused_beside_a_dry_run_or_a_filter() {
        let parse = |args: &[&str]| {
            SyncCommand::try_parse_from(std::iter::once("sync").chain(args.iter().copied()))
        };
        for args in [
            &["--reset", "--dry-run"][..],
            &["--reset", "-m", "INBOX"],
            &["--reset", "-x", "INBOX"],
            &["--reset", "-s", "imap"],
            &["--force"],
            &["--reset", "--force", "--dry-run"],
        ] {
            assert!(parse(args).is_err(), "{args:?} is refused");
        }
        assert!(parse(&["--reset"]).is_ok());
        assert!(parse(&["--reset", "--force"]).is_ok());
    }

    /// `account` syncing one IMAP source, as the store sees it.
    fn namespaced(account: &str) -> AccountConfig {
        let config: AccountConfig = toml::from_str("imap.server = \"imap.example.org\"").unwrap();
        config.namespaced(account).unwrap()
    }

    /// The inbox of `account`, as the store keys it.
    fn inbox(account: &str) -> String {
        format!("{account}/imap/INBOX")
    }

    /// Initializes `account` in the store at `dir` and files a message
    /// into its inbox, as a sync lands one: bound to its source with the
    /// body as its base, so it is no work a server lacks, unless `based`
    /// is false, a create staged and never pushed.
    fn land(dir: &Path, account: &str, id: &str, based: bool) {
        let config = namespaced(account);
        if !StoreState::load(dir).unwrap().initialized(account) {
            StoreState::stamp(dir, account, &config.mode().unwrap()).unwrap();
        }

        let body = format!("Message-ID: <{id}>\r\nSubject: {id}\r\n\r\nHello\r\n").into_bytes();
        let mut store = PimdirStore::open(dir)
            .unwrap()
            .for_account(account)
            .for_source(format!("{account}/imap"));
        store
            .ensure_collection(inbox(account), "message/rfc822")
            .unwrap();
        let derived = summary::derive("message/rfc822", &body).unwrap();
        let hash = store.hash(&body);
        let base = based.then(|| PimdirBase {
            flags: PimdirFlags::default(),
            revision: None,
            object: Some(hash.clone()),
        });
        store
            .write(vec![
                PimdirWriteOp::StoreObject {
                    object: PimdirObject {
                        hash: hash.clone(),
                        size: body.len(),
                    },
                    body: Some(body),
                },
                PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                    collection: PimdirCollectionId(inbox(account)),
                    handle: PimdirHandle(id.into()),
                    link_id: Some(PimdirLinkId(derived.link_id.0.clone())),
                    object: Some(hash),
                    level: PimdirLevel::Full,
                    summary: derived.summary,
                    sort_key: PimdirSortKey(derived.sort_key.0),
                    flags: PimdirFlags::default(),
                    status: PimdirStatus::Clean,
                    conflict_revision: None,
                    conflict_object: None,
                    base,
                    origin: None,
                }),
            ])
            .unwrap();
        drop(store);

        driver::prepare(dir, account, &config, false).unwrap();
    }

    /// How many capability rows `source` declares in the store at `dir`.
    fn declared(dir: &Path, source: &str) -> i64 {
        rusqlite::Connection::open(dir.join("pimdir.db"))
            .unwrap()
            .query_row(
                "SELECT count(*) FROM capabilities WHERE source = ?1",
                [source],
                |row| row.get(0),
            )
            .unwrap()
    }

    /// A reset starts one account over inside the store it shares: its
    /// collections, items and capabilities go and its sidecar entry starts
    /// again, while the other account keeps every one of its own.
    #[test]
    fn a_reset_drops_one_account_and_leaves_the_other() {
        let _owning = SPAWNS.read().unwrap_or_else(PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        land(dir.path(), "work", "w1@x", true);
        land(dir.path(), "home", "h1@x", true);
        let mut sidecar = StoreState::load(dir.path()).unwrap();
        for state in sidecar.accounts.values_mut() {
            state.mode_accepted = true;
        }
        sidecar.save(dir.path()).unwrap();

        reset_account(dir.path(), "work", &namespaced("work"), false).unwrap();

        let reader = PimdirReader::open(dir.path()).unwrap();
        assert!(
            reader
                .list_collections_by_account(Some("work"))
                .unwrap()
                .is_empty()
        );
        let home = reader.list_collections_by_account(Some("home")).unwrap();
        assert_eq!(home.len(), 1);
        assert_eq!(reader.list_items(inbox("home"), None, 10).unwrap().len(), 1);
        assert_eq!(declared(dir.path(), "work/imap"), 0);
        assert!(declared(dir.path(), "home/imap") > 0);

        let sidecar = StoreState::load(dir.path()).unwrap();
        assert!(!sidecar.account("work").unwrap().mode_accepted);
        assert!(sidecar.account("home").unwrap().mode_accepted);
    }

    /// A queued or parked action on the account's collections refuses the
    /// reset, forced or not, naming the row and the way out: the store
    /// would drop the row with its collection, and its producer never
    /// learn why.
    #[test]
    fn a_reset_refuses_while_an_action_is_queued() {
        let _owning = SPAWNS.read().unwrap_or_else(PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        land(dir.path(), "work", "w1@x", true);
        let remove = PimdirAction::Remove {
            seq: 1,
            if_match: None,
            resolves: None,
        };
        PimdirProducer::open(dir.path(), "tests")
            .unwrap()
            .enqueue(&inbox("work"), &remove, None)
            .unwrap();

        for force in [false, true] {
            let err = reset_account(dir.path(), "work", &namespaced("work"), force).unwrap_err();

            let err = format!("{err:#}");
            assert!(err.contains("queued action #1 (remove)"), "{err}");
            assert!(err.contains("neverest drain -a work"), "{err}");
        }
        let reader = PimdirReader::open(dir.path()).unwrap();
        assert_eq!(reader.list_pending_actions().unwrap().len(), 1);
        assert_eq!(reader.list_collections().unwrap().len(), 1);
    }

    /// Work no server has refuses a plain reset, naming `--force`, which
    /// drops it with the account's collections.
    #[test]
    fn a_forced_reset_drops_staged_work() {
        let _owning = SPAWNS.read().unwrap_or_else(PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        land(dir.path(), "work", "w1@x", false);

        let err = reset_account(dir.path(), "work", &namespaced("work"), false).unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("1 unpushed change"), "{err}");
        assert!(err.contains("--force"), "{err}");
        assert_eq!(
            PimdirReader::open(dir.path())
                .unwrap()
                .list_collections()
                .unwrap()
                .len(),
            1
        );

        reset_account(dir.path(), "work", &namespaced("work"), true).unwrap();

        let reader = PimdirReader::open(dir.path()).unwrap();
        assert!(reader.list_collections().unwrap().is_empty());
        StoreState::load(dir.path())
            .unwrap()
            .account("work")
            .unwrap();
    }

    /// A delivered create is a staged create still: its server took it and
    /// no listing named it yet (pimdir SYNC §2), so its base names nothing
    /// and what is staged on it is in the store alone. A plain reset
    /// refuses over it as over a pending one.
    #[test]
    fn a_reset_refuses_while_a_delivered_create_waits() {
        let _owning = SPAWNS.read().unwrap_or_else(PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        land(dir.path(), "work", "w1@x", true);

        let key = PimdirLinkId("w2@x".into());
        let mut store = PimdirStore::open(dir.path())
            .unwrap()
            .for_account("work")
            .for_source("work/imap");
        store
            .write(vec![
                PimdirWriteOp::UpsertPlacement(PimdirPlacement {
                    collection: PimdirCollectionId(inbox("work")),
                    handle: key.provisional(),
                    link_id: Some(key.clone()),
                    object: None,
                    level: PimdirLevel::Meta,
                    summary: None,
                    sort_key: PimdirSortKey::default(),
                    flags: PimdirFlags::from_iter(["\\Seen"]),
                    status: PimdirStatus::Created,
                    conflict_revision: None,
                    conflict_object: None,
                    base: None,
                    origin: None,
                }),
                PimdirWriteOp::DeliverPlacement {
                    collection: PimdirCollectionId(inbox("work")),
                    handle: key.provisional(),
                },
            ])
            .unwrap();
        drop(store);

        let err = reset_account(dir.path(), "work", &namespaced("work"), false).unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("1 unpushed change"), "{err}");
    }

    /// A reset starts over an account `init` initialized, never one it did
    /// not: that is `init`'s, which opens every endpoint first.
    #[test]
    fn a_reset_needs_an_initialized_account() {
        let _owning = SPAWNS.read().unwrap_or_else(PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        land(dir.path(), "work", "w1@x", true);

        let err = reset_account(dir.path(), "home", &namespaced("home"), true).unwrap_err();

        assert!(format!("{err:#}").contains("neverest init -a home"));
    }
}
