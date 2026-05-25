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

//! `neverest init` command: probes both sides and writes the initial
//! cache snapshot that subsequent sync runs consume.
//!
//! The cache file's presence is the single source of truth for "this
//! account is initialized".

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use pimalaya_cli::{
    clap::args::AccountFlag,
    printer::{Message, Printer},
    spinner::Spinner,
};
use pimalaya_config::toml::TomlConfig;

use crate::{client, config::Config, sync::cache::CacheSnapshot};

/// Initializes an account's per-side state; refuses to run if it is
/// already initialized.
#[derive(Debug, Parser)]
pub struct InitCommand {
    #[command(flatten)]
    pub account: AccountFlag,
}

impl InitCommand {
    pub fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        let mut config = Config::load_or_wizard(config_paths)?;

        let account_name = self.account.name.as_deref();
        let Some((name, account_config)) = config.take_account(account_name)? else {
            bail!("Cannot find account");
        };

        let cache = CacheSnapshot::path(&name)?;
        if cache.exists() {
            let p = cache.display();
            bail!("Account `{name}` already initialized, delete `{p}` to reset");
        }

        let s = Spinner::start("Initializing left side…");
        client::init(account_config.left.clone()).context("Initialize left side")?;
        s.success("Initialized left side");

        let s = Spinner::start("Initializing right side…");
        client::init(account_config.right.clone()).context("Initialize right side")?;
        s.success("Initialized right side");

        let s = Spinner::start("Writing initial cache snapshot…");
        CacheSnapshot::default()
            .save(&cache)
            .context(format!("Write initial cache `{}`", cache.display()))?;
        s.success("Wrote initial cache snapshot");

        printer.out(Message::new(format!(
            "Account `{name}` successfully initialized"
        )))
    }
}
