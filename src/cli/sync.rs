//! `neverest sync` command: runs the io-replica-based reconcile and prints the
//! resulting [`crate::sync::report::SyncReport`].

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
    config::{CollectionFilter, Config},
    offline::driver,
};

/// How long a run waits for another run's store lock before giving up:
/// long enough for a connector-triggered scoped run to queue behind a
/// cron tick, short enough to fail loudly on a wedged holder.
const LOCK_TIMEOUT: Duration = Duration::from_secs(60);

/// How often the waiter retries the lock.
const LOCK_POLL: Duration = Duration::from_millis(500);

/// Synchronizes the account's collections and items through its pimdir store.
///
/// Every source of one kind sharing a `collection.namespace` is reconciled
/// against one set of hub collections: alone, that is the offline replica; as a
/// pair, a mirror. Namespaces never meet, so a mail source and a contacts
/// source under one account stay apart.
///
/// The three filter flags keep their pre-`generic-pim-sync` spellings
/// (`--include-mailbox`, `--exclude-mailbox`, `--all-mailboxes`) as hidden
/// aliases, so existing scripts and the connector contract keep working.
#[derive(Debug, Parser)]
pub struct SyncCommand {
    /// Run the synchronization without applying any changes; only
    /// prints the patch that would have been applied.
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

    /// Synchronize only the namespaces holding the given sources (repeatable).
    ///
    /// Narrowing picks namespaces rather than sources: two sources sharing one
    /// are a mirror, and running half of a mirror pushes one way and calls it
    /// done.
    #[arg(long, short = 's', value_name = "SOURCE", action = ArgAction::Append)]
    pub source: Vec<String>,
}

impl SyncCommand {
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        account_name: Option<&str>,
    ) -> Result<()> {
        let mut config = Config::load_or_wizard(printer, config_paths)?;

        let Some((name, account_config)) = config.take_account(account_name)? else {
            bail!("Cannot find account");
        };

        account_config.validate()?;

        let replica = driver::store_dir(&name, &account_config)?;
        if !replica.join("pimdir.db").exists() {
            bail!("Account `{name}` not initialized, run `init -a {name}` first");
        }

        let _sync_lock = acquire_store_lock(&replica, LOCK_TIMEOUT)
            .with_context(|| format!("Acquire the store lock of account `{name}`"))?;

        if self.reset {
            reset_replica(&replica, &self.include_collection)?;
            info!("reset: dropped replica state for `{name}`");
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
        )?;

        printer.out(report)
    }
}

/// Takes the store's advisory `sync.lock` for the whole run, waiting up
/// to `timeout` for another run to release it (cron ticks and
/// connector-triggered scoped runs serialize instead of failing), then
/// erroring out clearly. The kernel releases the lock on FD close
/// (normal exit or crash), so no PID file to clean up.
fn acquire_store_lock(store_dir: &Path, timeout: Duration) -> Result<File> {
    let lock_path = store_dir.join("sync.lock");
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("Open sync lock `{}` error", lock_path.display()))?;

    let deadline = Instant::now() + timeout;
    let mut waiting = false;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) => {
                if Instant::now() >= deadline {
                    bail!(
                        "Another sync still holds `{}` after {}s",
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
                    .with_context(|| format!("Acquire sync lock `{}` error", lock_path.display()));
            }
        }
    }
}

/// Drops the pimdir store (items, bindings, checkpoints) and blobs so the next
/// sync re-reconciles from scratch. A scoped reset (`--include-collection`) is not
/// yet supported by the store; it drops everything.
fn reset_replica(replica: &std::path::Path, include: &[String]) -> Result<()> {
    if !include.is_empty() {
        info!("reset: per-collection scope not yet supported, resetting whole replica");
    }
    // NOTE: the JSON files are the pre-pimdir layouts, removed so an upgraded
    // install resets cleanly.
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
                .with_context(|| format!("Remove `{}` for reset", path.display()))?;
        }
    }
    let objects = replica.join("objects");
    if objects.exists() {
        fs::remove_dir_all(&objects)
            .with_context(|| format!("Remove `{}` for reset", objects.display()))?;
    }
    // NOTE: the empty store is recreated so the account stays initialized.
    PimdirStore::open(replica).context("Recreate pimdir store after reset")?;
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
}
