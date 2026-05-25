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

//! `neverest configure` command: re-runs the wizard against an existing
//! account, using current values as defaults, and saves the result.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Parser;
use pimalaya_cli::{
    clap::args::AccountFlag,
    printer::{Message, Printer},
};
use pimalaya_config::toml::TomlConfig;

use crate::{config::Config, wizard::edit::edit_account};

#[derive(Debug, Parser)]
pub struct ConfigureCommand {
    #[command(flatten)]
    pub account: AccountFlag,
}

impl ConfigureCommand {
    pub fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        let config = Config::load_or_wizard(config_paths)?;

        let name = match self.account.name.as_deref() {
            Some(name) => name.to_owned(),
            None => {
                let mut probe = config.clone();

                let Some((name, _)) = probe.take_account(None)? else {
                    bail!("Cannot find default account");
                };

                name
            }
        };

        let target = Config::target_path(config_paths)?;
        edit_account(&target, config, &name)?;

        printer.out(Message::new(format!("Account `{name}` configured")))
    }
}
