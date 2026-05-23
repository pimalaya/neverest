//! `neverest configure` command.
//!
//! Re-runs the wizard against an existing account: re-prompts for the
//! left and right sides (IMAP/JMAP credentials, m2dir store root)
//! using the current values as defaults, then writes the updated
//! [`Config`] back to disk. Account selection follows the standard
//! `--account` flag with the `default = true` fallback.

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

        // Resolve the account name up-front (either the `--account` flag or the
        // entry flagged `default = true`) before handing off to `edit_account`,
        // which expects an existing name and re-uses the matching account's
        // values as wizard defaults.
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
