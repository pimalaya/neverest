//! # Sync command
//!
//! Runs the io-pimdir reconcile and prints the resulting
//! [`crate::sync::report::SyncOutput`].

use std::{
    fs::{self, File, TryLockError},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use clap::{ArgAction, Parser};
use io_pimdir::{
    client::{PimdirStore, modes},
    object::PimdirHash,
    placement::{PimdirBase, PimdirFlags},
};
use log::{debug, info};
use pimalaya_cli::printer::Printer;
use pimalaya_config::toml::TomlConfig;

use crate::{
    cli::exit::Exit,
    config::{CollectionFilter, Config},
    error::pimdir_error,
    offline::{
        driver::{self, Bodies, DownloadOrder, Pacing},
        state::StoreState,
    },
};

/// How long a run waits for another run's store lock before giving up.
///
/// Long enough for a connector-triggered scoped run to queue behind a cron
/// tick, short enough to fail loudly on a wedged holder.
pub const LOCK_TIMEOUT: Duration = Duration::from_secs(60);

/// How often the waiter retries the lock.
const LOCK_POLL: Duration = Duration::from_millis(500);

/// Synchronizes the account's collections and items through its pimdir store.
///
/// Each source is reconciled against the store, then against every target the
/// account names. Sources never meet: an item one holds crosses to the
/// targets, never to another source, so a mail source and a contacts source
/// under one account stay apart.
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
/// intent that failed without parking, or a collection still moving when the
/// run stopped. It wins over 2.
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
    /// Drop the store before running, then resync it whole.
    ///
    /// Refused while the store holds work no server has (queued or parked
    /// actions, conflicts, unpushed changes, retained items), which the
    /// refusal lists; `--force` moves the store aside instead.
    #[arg(long, conflicts_with_all = [
        "dry_run",
        "include_collection",
        "exclude_collection",
        "source",
    ])]
    pub reset: bool,
    /// With `--reset`, move the store into a sibling `<dir>.reset-<instant>`
    /// directory rather than dropping it, whatever it holds: nothing is
    /// deleted.
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
    /// Reconciles the account, or plans the run under `--dry-run`.
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        account_name: Option<&str>,
    ) -> Result<Exit> {
        let mut config = Config::load_or_wizard(printer, config_paths)?;

        let Some((name, account_config)) = config.take_account(account_name)? else {
            bail!("Cannot find account");
        };

        account_config.validate()?;

        let replica = driver::store_dir(&name, &account_config)?;
        if !replica.join("pimdir.db").exists() {
            bail!("Account {name} not initialized, run `init -a {name}` first");
        }

        let _sync_lock = acquire_store_lock(&replica, LOCK_TIMEOUT)
            .with_context(|| format!("Acquire the store lock of account {name}"))?;

        if self.reset {
            reset_replica(&replica, self.force)
                .with_context(|| format!("Reset the store of account {name}"))?;
        }

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

        let scope = account_config.scope(self.since.as_deref(), &self.source, Utc::now())?;

        let report = driver::run(
            &name,
            &account_config,
            cli_filter,
            &scope,
            self.dry_run,
            connections,
            self.no_purge,
            &self.source,
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
        )?;

        // NOTE: read before printing, which consumes the report.
        let exit = Exit::from(&report);
        printer.out(report)?;

        Ok(exit)
    }
}

/// Takes the store's advisory sync.lock for the whole run.
///
/// Waits up to `timeout` so cron ticks and connector-triggered scoped runs
/// serialize instead of failing, then errors out. The kernel releases the
/// lock on FD close, so there is no PID file to clean up.
pub fn acquire_store_lock(store_dir: &Path, timeout: Duration) -> Result<File> {
    let (lock_path, file) = open_store_lock(store_dir)?;

    let deadline = Instant::now() + timeout;
    let mut waiting = false;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    bail!(
                        "Another sync still holds {} after {}s",
                        lock_path.display(),
                        timeout.as_secs()
                    );
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

/// Opens, creating it if needed, the file the sync.lock is taken on.
fn open_store_lock(store_dir: &Path) -> Result<(PathBuf, File)> {
    let lock_path = store_dir.join("sync.lock");
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("Open sync lock {} error", lock_path.display()))?;
    Ok((lock_path, file))
}

/// Drops the pimdir store and its bodies so the next sync re-reconciles, or
/// with `force` moves them aside.
///
/// A store holding work no server has is refused unless forced, the refusal
/// naming that work, and so is a store that cannot be read, its work being
/// unknown. A forced reset deletes nothing: every file of the store but its
/// lock moves into a sibling `<dir>.reset-<instant>` directory.
fn reset_replica(replica: &Path, force: bool) -> Result<()> {
    if force {
        let aside = move_aside(replica)?;
        info!("reset: moved the store aside to {}", aside.display());
    } else {
        let pending = pending_work(replica).context(
            "Cannot read the store to check its pending work; \
             `sync --reset --force` moves it aside instead",
        )?;
        if !pending.is_empty() {
            bail!(
                "Cannot reset a store holding work no server has:\n{}\n\
                 `sync --reset --force` moves the store aside instead",
                pending.join("\n")
            );
        }
        drop_store(replica)?;
        info!("reset: dropped the store");
    }

    PimdirStore::open(replica)
        .map_err(pimdir_error)
        .context("Recreate pimdir store after reset")?;
    StoreState::stamp(replica, None).context("Stamp store state after reset")?;
    Ok(())
}

/// What the store holds that no server has, one line per kind of work: its
/// queued and parked actions, then per collection its conflicts, its
/// retained items (bound to no server), its pending deletes and the items
/// whose state no binding's base holds.
fn pending_work(replica: &Path) -> Result<Vec<String>> {
    let store = PimdirStore::open(replica).map_err(pimdir_error)?;
    let mut pending = Vec::new();

    let queued = store.list_pending_actions()?.len();
    if queued > 0 {
        pending.push(format!("- {queued} queued action(s)"));
    }
    let parked = store.parked_actions()?.len();
    if parked > 0 {
        pending.push(format!("- {parked} parked action(s)"));
    }

    for collection in store.list_collections()? {
        let hub = store.load_hub(&collection.id)?;
        let (mut conflicts, mut retained, mut deletes, mut unpushed) = (0, 0, 0, 0);

        for item in hub.items.values() {
            let held = item.sources.values().any(|binding| {
                binding
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

/// Removes the store's database, its search index and its bodies.
fn drop_store(replica: &Path) -> Result<()> {
    for name in [
        "pimdir.db",
        "pimdir.db-wal",
        "pimdir.db-shm",
        "index.db",
        "index.db-wal",
        "index.db-shm",
        "replica.json",
        "index.json",
        "links.json",
    ] {
        let path = replica.join(name);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Remove {} for reset", path.display()))?;
        }
    }
    let objects = replica.join("objects");
    if objects.exists() {
        fs::remove_dir_all(&objects)
            .with_context(|| format!("Remove {} for reset", objects.display()))?;
    }
    Ok(())
}

/// Moves every file of the store but its lock into a sibling directory named
/// after it and the instant, answering where.
fn move_aside(replica: &Path) -> Result<PathBuf> {
    let name = replica
        .file_name()
        .with_context(|| format!("Name the store directory {}", replica.display()))?
        .to_string_lossy();
    let instant = Utc::now().format("%Y%m%dT%H%M%SZ");
    let aside = replica.with_file_name(format!("{name}.reset-{instant}"));

    // NOTE: the moved store stays its user's alone, as it was (pimdir STORAGE
    // §18), so it opens again as a store.
    modes::create_dir(&aside).with_context(|| format!("Create {}", aside.display()))?;
    let entries = fs::read_dir(replica).with_context(|| format!("Read {}", replica.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("Read {}", replica.display()))?;
        if entry.file_name() == "sync.lock" {
            continue;
        }
        let to = aside.join(entry.file_name());
        fs::rename(entry.path(), &to)
            .with_context(|| format!("Move {} to {}", entry.path().display(), to.display()))?;
    }

    Ok(aside)
}

#[cfg(test)]
mod tests {
    use io_pimdir::{
        client::{producer::PimdirProducer, reader::PimdirReader},
        codec::PimdirAction,
    };

    use super::*;

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

    #[test]
    fn a_second_run_waits_out_the_lock_then_gives_up() {
        let dir = tempfile::tempdir().unwrap();

        let held = acquire_store_lock(dir.path(), Duration::from_millis(1)).unwrap();

        let start = Instant::now();
        let err = acquire_store_lock(dir.path(), Duration::from_millis(50)).unwrap_err();
        assert!(start.elapsed() >= Duration::from_millis(50));
        assert!(format!("{err:#}").contains("Another sync still holds"));

        drop(held);
        acquire_store_lock(dir.path(), Duration::from_millis(1)).unwrap();
    }

    /// The refusal a sidecar-less store raises names `--reset` as its
    /// remedy, so a reset leaving it sidecar-less would raise it again.
    #[test]
    fn a_reset_leaves_a_store_the_next_run_can_read() {
        let dir = tempfile::tempdir().unwrap();

        reset_replica(dir.path(), false).unwrap();

        assert!(dir.path().join("pimdir.db").exists());
        StoreState::load(dir.path()).unwrap();
    }

    /// A reset is the whole store's: beside a dry run it would drop the
    /// real store before planning, and beside a filter it would reset more
    /// than it names. `--force` means nothing without it.
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

    /// A store holding a queued action, as a frontend leaves one.
    fn store_with_queued_action(dir: &Path) {
        PimdirStore::open(dir)
            .unwrap()
            .for_account("a")
            .for_source("imap")
            .ensure_collection("imap/INBOX", "message/rfc822")
            .unwrap();
        StoreState::stamp(dir, None).unwrap();
        PimdirProducer::open(dir, "tests")
            .unwrap()
            .enqueue("imap/INBOX", &PimdirAction::Remove { seq: 1 }, None)
            .unwrap();
    }

    /// A reset never drops work no server has: it refuses, names what is
    /// pending and `--force`, and leaves the store as it was.
    #[test]
    fn a_reset_refuses_while_work_is_pending() {
        let dir = tempfile::tempdir().unwrap();
        store_with_queued_action(dir.path());

        let err = reset_replica(dir.path(), false).unwrap_err();

        let err = format!("{err:#}");
        assert!(err.contains("1 queued action"), "{err}");
        assert!(err.contains("--force"), "{err}");
        let reader = PimdirReader::open(dir.path()).unwrap();
        assert_eq!(reader.list_pending_actions().unwrap().len(), 1);
    }

    /// A forced reset moves the store aside, whole, and starts a new one.
    #[test]
    fn a_forced_reset_moves_the_store_aside() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("account");
        fs::create_dir_all(&dir).unwrap();
        store_with_queued_action(&dir);

        reset_replica(&dir, true).unwrap();

        let aside: Vec<PathBuf> = fs::read_dir(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path != &dir)
            .collect();
        assert_eq!(aside.len(), 1, "{aside:?}");
        let name = aside[0].file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("account.reset-"), "{name}");
        let kept = PimdirReader::open(&aside[0]).unwrap();
        assert_eq!(kept.list_pending_actions().unwrap().len(), 1);

        let fresh = PimdirReader::open(&dir).unwrap();
        assert!(fresh.list_pending_actions().unwrap().is_empty());
        StoreState::load(&dir).unwrap();
    }

    /// The search index goes the way of the store it was built from: a
    /// reset drops it with an idle store, a forced one moves it aside with
    /// the store, so no index outlives the store its stamps count in.
    #[cfg(feature = "search")]
    #[test]
    fn a_reset_takes_the_search_index_with_the_store() {
        use io_pimdir::client::search::{PimdirIndex, PimdirIndexRefreshOptions};

        let index = |dir: &Path| {
            PimdirIndex::open(dir)
                .unwrap()
                .refresh(&PimdirIndexRefreshOptions::default())
                .unwrap();
        };

        let root = tempfile::tempdir().unwrap();
        let idle = root.path().join("idle");
        fs::create_dir_all(&idle).unwrap();
        reset_replica(&idle, false).unwrap();
        index(&idle);
        reset_replica(&idle, false).unwrap();
        assert!(!idle.join("index.db").exists(), "dropped with the store");

        let busy = root.path().join("busy");
        fs::create_dir_all(&busy).unwrap();
        store_with_queued_action(&busy);
        index(&busy);
        reset_replica(&busy, true).unwrap();
        assert!(
            !busy.join("index.db").exists(),
            "not left beside a new store"
        );
        let aside: Vec<PathBuf> = fs::read_dir(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.to_string_lossy().contains("busy.reset-"))
            .collect();
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert!(
            aside[0].join("index.db").exists(),
            "moved aside, not deleted"
        );
    }
}
