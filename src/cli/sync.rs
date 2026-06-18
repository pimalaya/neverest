//! `neverest sync` command: opens the worker pool, runs the sync and
//! prints the resulting [`crate::sync::report::SyncReport`].

use std::{
    fs::{File, TryLockError},
    path::PathBuf,
};

use anyhow::{Context, Result, bail};
use clap::{ArgAction, Parser};
use log::info;
use pimalaya_cli::{clap::args::AccountFlag, printer::Printer, spinner::Spinner};
use pimalaya_config::toml::TomlConfig;

use crate::{
    config::{Config, MailboxFilter},
    sync::{self, pool::Pool, state::StateSnapshot},
};

/// Synchronizes mailboxes and messages between the configured left and
/// right sides.
#[derive(Debug, Parser)]
pub struct SyncCommand {
    #[command(flatten)]
    pub account: AccountFlag,

    /// Run the synchronization without applying any changes; only
    /// prints the patch that would have been applied.
    #[arg(long, short = 'd')]
    pub dry_run: bool,

    /// Synchronize only the given mailbox names (repeatable, ASCII
    /// case-insensitive).
    #[arg(long, short = 'm')]
    #[arg(value_name = "MAILBOX", action = ArgAction::Append)]
    #[arg(conflicts_with = "exclude_mailbox", conflicts_with = "all_mailboxes")]
    pub include_mailbox: Vec<String>,

    /// Skip the given mailbox names (repeatable, ASCII case-insensitive).
    #[arg(long, short = 'x')]
    #[arg(value_name = "MAILBOX", action = ArgAction::Append)]
    #[arg(conflicts_with = "include_mailbox", conflicts_with = "all_mailboxes")]
    pub exclude_mailbox: Vec<String>,

    /// Synchronize every mailbox on both sides, ignoring config filters.
    #[arg(long, short = 'A')]
    #[arg(conflicts_with = "include_mailbox", conflicts_with = "exclude_mailbox")]
    pub all_mailboxes: bool,

    /// Drop the persisted sync state before running; restricted to
    /// `--include-mailbox` entries when set.
    #[arg(long)]
    pub reset: bool,
}

impl SyncCommand {
    pub fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        let mut config = Config::load_or_wizard(config_paths)?;

        let account_name = self.account.name.as_deref();
        let Some((name, account_config)) = config.take_account(account_name)? else {
            bail!("Cannot find account");
        };

        let state = StateSnapshot::path(&name)?;
        if !state.exists() {
            bail!("Account `{name}` not initialized, run `init -a {name}` first");
        }

        // NOTE: advisory flock held for the whole sync; the kernel
        // releases it on FD close (normal exit or crash), so no PID
        // file to clean up.
        let lock_path = state.with_file_name("sync.lock");
        let _sync_lock = {
            let file = File::options()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)
                .context(format!("Open sync lock `{}` error", lock_path.display()))?;

            match file.try_lock() {
                Ok(()) => {}
                Err(TryLockError::WouldBlock) => {
                    bail!("Another sync is already running for account `{name}`");
                }
                Err(TryLockError::Error(err)) => {
                    return Err(err)
                        .context(format!("Acquire sync lock `{}` error", lock_path.display()));
                }
            }

            file
        };

        if self.reset {
            let mut snapshot = StateSnapshot::load(&state)?;
            snapshot.resync(&self.include_mailbox);
            snapshot
                .save(&state)
                .context(format!("Clear state `{}` for --resync", state.display()))?;
            if self.include_mailbox.is_empty() {
                info!("resync: cleared state for `{name}`");
            } else {
                info!(
                    "resync: cleared {} mailbox(es) for `{name}`",
                    self.include_mailbox.len()
                );
            }
        }

        let s = Spinner::start("Opening worker pool…");
        let pool = Pool::open(account_config.left.clone(), account_config.right.clone())?;
        s.success(format!(
            "Opened worker pool ({} left, {} right)",
            pool.left.len(),
            pool.right.len()
        ));

        let cli_filter = if !self.include_mailbox.is_empty() {
            Some(MailboxFilter::Include(self.include_mailbox.clone()))
        } else if !self.exclude_mailbox.is_empty() {
            Some(MailboxFilter::Exclude(self.exclude_mailbox.clone()))
        } else if self.all_mailboxes {
            Some(MailboxFilter::All)
        } else {
            None
        };

        let report = sync::run(&name, &account_config, pool, cli_filter, self.dry_run)?;

        printer.out(report)
    }
}
