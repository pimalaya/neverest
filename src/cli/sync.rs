//! # Sync command
//!
//! Runs the io-replica-based reconcile and prints the resulting
//! [`crate::sync::report::SyncOutput`].

use std::{
    fs::{self, File, TryLockError},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use clap::{ArgAction, Parser};
use io_pimdir::PimdirStore;
use log::{debug, info};
use pimalaya_cli::printer::Printer;
use pimalaya_config::toml::TomlConfig;

use crate::{
    cli::exit::Exit,
    config::{CollectionFilter, Config},
    offline::{driver, state::StoreState},
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
/// refuses, or a write it would not take. None is a failure: each is one item
/// wide and halts nothing, and under a supervisor restarting on failure they
/// would loop over a state no supervisor can fix.
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

    /// Drop the persisted replica before running (a full re-reconcile).
    #[arg(long)]
    pub reset: bool,

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
}

impl SyncCommand {
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
            reset_replica(&replica, &self.include_collection)?;
            info!("reset: dropped replica state for {name}");
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

        let report = driver::run(
            &name,
            &account_config,
            cli_filter,
            self.dry_run,
            connections,
            self.no_purge,
            &self.source,
            self.accept_mode,
        )?;

        // Read before printing, which consumes the report.
        let exit = Exit::from(&report);
        printer.out(report)?;

        Ok(exit)
    }
}

/// Takes the store's advisory `sync.lock` for the whole run.
///
/// Waits up to `timeout` so cron ticks and connector-triggered scoped runs
/// serialize instead of failing, then errors out. The kernel releases the
/// lock on FD close, so there is no PID file to clean up.
pub fn acquire_store_lock(store_dir: &Path, timeout: Duration) -> Result<File> {
    let lock_path = store_dir.join("sync.lock");
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("Open sync lock {} error", lock_path.display()))?;

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

/// Drops the pimdir store and blobs so the next sync re-reconciles.
///
/// A scoped reset (`--include-collection`) is not yet supported by the
/// store; it drops everything.
fn reset_replica(replica: &std::path::Path, include: &[String]) -> Result<()> {
    if !include.is_empty() {
        info!("reset: per-collection scope not yet supported, resetting whole replica");
    }
    for name in [
        "pimdir.db",
        "pimdir.db-wal",
        "pimdir.db-shm",
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
    PimdirStore::open(replica).context("Recreate pimdir store after reset")?;
    StoreState::stamp(replica, None).context("Stamp store state after reset")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

        reset_replica(dir.path(), &[]).unwrap();

        assert!(dir.path().join("pimdir.db").exists());
        StoreState::load(dir.path()).unwrap();
    }
}
