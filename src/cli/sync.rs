// This file is part of Neverest, a CLI to synchronize emails.
//
// Copyright (C) 2024-2026  soywod <pimalaya.org@posteo.net>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! `neverest sync` command: opens the worker pool, runs the sync and
//! prints the resulting [`crate::sync::report::SyncReport`].

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{ArgAction, Parser};
use log::info;
use pimalaya_cli::{clap::args::AccountFlag, printer::Printer, spinner::Spinner};
use pimalaya_config::toml::TomlConfig;

use crate::{
    config::{Config, MailboxFilter},
    sync::{self, cache::CacheSnapshot, pool::Pool},
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

    /// Drop the cached sync state before running; restricted to
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

        let cache = CacheSnapshot::path(&name)?;
        if !cache.exists() {
            bail!("Account `{name}` not initialized, run `init {name}` first");
        }

        if self.reset {
            let mut snapshot = CacheSnapshot::load(&cache)?;
            snapshot.resync(&self.include_mailbox);
            snapshot
                .save(&cache)
                .context(format!("Clear cache `{}` for --resync", cache.display()))?;
            if self.include_mailbox.is_empty() {
                info!("resync: cleared cache for `{name}`");
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
