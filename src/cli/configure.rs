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

/// Re-runs the wizard over an existing account, seeded with its current
/// values, and saves the result.
#[derive(Debug, Parser)]
pub struct ConfigureCommand {
    #[command(flatten)]
    pub account: AccountFlag,
}

impl ConfigureCommand {
    pub fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        let config = Config::load_or_wizard(printer, config_paths)?;

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
