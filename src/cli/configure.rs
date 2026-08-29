//! # Configure command
//!
//! Re-runs the wizard against an existing account, using current values as
//! defaults, and saves the result.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Parser;
use pimalaya_cli::printer::{Message, Printer};
use pimalaya_config::toml::TomlConfig;

use crate::{config::Config, wizard::edit::edit_account};

/// Re-runs the wizard over an existing account, seeded with its current
/// values, and saves the result.
#[derive(Debug, Parser)]
pub struct ConfigureCommand {}

impl ConfigureCommand {
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        account_name: Option<&str>,
    ) -> Result<()> {
        let config = Config::load_or_wizard(printer, config_paths)?;

        let name = match account_name {
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

        printer.out(Message::new(format!("Account {name} configured")))
    }
}
