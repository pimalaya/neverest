//! `neverest sync` command.
//!
//! Loads the per-account config, opens both side pools (each pool
//! opens its N clients in parallel), invokes [`crate::sync::engine::run`],
//! then hands the resulting [`crate::sync::report::SyncReport`] to the
//! printer. The report is both [`fmt::Display`] and
//! [`serde::Serialize`], so terminal and `--json` output are the same
//! call-site.
//!
//! In-flight progress is surfaced via `log::info!` (stage transitions,
//! per-mailbox start) and `log::debug!` (per-hunk apply) from inside
//! the engine.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{ArgAction, Parser};
use log::info;
use pimalaya_cli::{clap::args::AccountFlag, printer::Printer, spinner::Spinner};
use pimalaya_config::toml::TomlConfig;

use crate::{
    config::{Config, MailboxFilter},
    side::Side,
    sync::{
        cache::{CacheSnapshot, cache_path},
        engine,
        pool::SidePool,
    },
};

/// Synchronize mailboxes and messages of the configured left and
/// right backends.
#[derive(Debug, Parser)]
pub struct SyncCommand {
    #[command(flatten)]
    pub account: AccountFlag,

    /// Run the synchronization without applying any changes.
    ///
    /// A report is printed describing the patch the synchronization
    /// would have applied.
    #[arg(long, short = 'd')]
    pub dry_run: bool,

    /// Synchronize only the given mailbox names. Repeat the flag for
    /// multiple names; conflicts with `--exclude-mailbox` and
    /// `--all-mailboxes`. Matching is ASCII case-insensitive: ASCII
    /// letters fold (`INBOX` matches `inbox`) but non-ASCII characters
    /// (umlauts, Cyrillic, accents) must be spelled exactly as the
    /// server reports them.
    #[arg(long, short = 'm')]
    #[arg(value_name = "MAILBOX", action = ArgAction::Append)]
    #[arg(conflicts_with = "exclude_mailbox", conflicts_with = "all_mailboxes")]
    pub include_mailbox: Vec<String>,

    /// Skip the given mailbox names. Repeat the flag for multiple
    /// names; conflicts with `--include-mailbox` and `--all-mailboxes`.
    /// Matching is ASCII case-insensitive (see `--include-mailbox` for
    /// the Unicode caveat).
    #[arg(long, short = 'x')]
    #[arg(value_name = "MAILBOX", action = ArgAction::Append)]
    #[arg(conflicts_with = "include_mailbox", conflicts_with = "all_mailboxes")]
    pub exclude_mailbox: Vec<String>,

    /// Synchronize every mailbox on both sides, ignoring any filter
    /// defined in the configuration file.
    #[arg(long, short = 'A')]
    #[arg(conflicts_with = "include_mailbox", conflicts_with = "exclude_mailbox")]
    pub all_mailboxes: bool,

    /// Drop the cached sync state before running. Without
    /// `--include-mailbox`, the entire snapshot + every IMAP/JMAP
    /// state token is cleared; with `--include-mailbox`, only the
    /// listed mailboxes are wiped. The first post-resync sync rebuilds
    /// the snapshot via a full re-list, equivalent to first-sync
    /// semantics.
    #[arg(long)]
    pub resync: bool,
}

impl SyncCommand {
    pub fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        let mut config = Config::load_or_wizard(config_paths)?;

        let account_name = self.account.name.as_deref();
        let Some((name, account_config)) = config.take_account(account_name)? else {
            bail!("Cannot find account");
        };

        let cache = cache_path(&name)?;
        if !cache.exists() {
            bail!("Account `{name}` not initialized, run `init {name}` first");
        }

        if self.resync {
            let mut snapshot = CacheSnapshot::load(&cache)?;
            snapshot.resync(&self.include_mailbox);
            snapshot
                .save(&cache)
                .context(format!("Clear cache `{}` for --resync", cache.display()))?;
            if self.include_mailbox.is_empty() {
                info!("resync: cleared every cached token + snapshot for account `{name}`");
            } else {
                info!(
                    "resync: cleared {} cached mailbox(es) for account `{name}`",
                    self.include_mailbox.len()
                );
            }
        }

        let s = Spinner::start("Opening left side…");
        let left = SidePool::open(account_config.left.clone(), Side::Left)?;
        s.success(format!("Opened left side ({} clients)", left.size()));

        let s = Spinner::start("Opening right side…");
        let right = SidePool::open(account_config.right.clone(), Side::Right)?;
        s.success(format!("Opened right side ({} clients)", right.size()));

        let cli_filter = if !self.include_mailbox.is_empty() {
            Some(MailboxFilter::Include(self.include_mailbox.clone()))
        } else if !self.exclude_mailbox.is_empty() {
            Some(MailboxFilter::Exclude(self.exclude_mailbox.clone()))
        } else if self.all_mailboxes {
            Some(MailboxFilter::All)
        } else {
            None
        };

        let report = engine::run(
            &name,
            &account_config,
            left,
            right,
            cli_filter,
            self.dry_run,
        )?;

        printer.out(report)
    }
}
