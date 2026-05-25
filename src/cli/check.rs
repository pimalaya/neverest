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

//! `neverest check` command: opens both sides and lists their mailboxes
//! to surface credential, network or config errors before a real sync.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Parser;
use log::info;
use pimalaya_cli::{
    clap::args::AccountFlag,
    printer::{Message, Printer},
    spinner::Spinner,
};
use pimalaya_config::toml::TomlConfig;

use crate::{
    client,
    config::{Config, SideConfig},
};

#[derive(Debug, Parser)]
pub struct CheckCommand {
    #[command(flatten)]
    pub account: AccountFlag,
}

impl CheckCommand {
    pub fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        let mut config = Config::load_or_wizard(config_paths)?;

        let account_name = self.account.name.as_deref();
        let Some((name, account_config)) = config.take_account(account_name)? else {
            bail!("Cannot find account");
        };

        info!("checking account `{name}`");
        check_side("left", account_config.left)?;
        check_side("right", account_config.right)?;

        printer.out(Message::new(format!("Account `{name}` looks healthy")))
    }
}

/// Opens the side and probes it with a `list_mailboxes` call.
fn check_side(label: &str, config: SideConfig) -> Result<()> {
    let s = Spinner::start(format!("Checking {label} side…"));
    let mut client = client::open(config)?;
    let mailboxes = client.list_mailboxes(false)?;
    s.success(format!(
        "Checked {label} side ({} mailboxes)",
        mailboxes.len()
    ));
    Ok(())
}
