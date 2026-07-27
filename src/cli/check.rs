//! `neverest check` command: opens both sides and lists their collections
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

/// Opens every configured side and lists their collections, surfacing
/// credential, network or config errors before a real sync.
#[derive(Debug, Parser)]
pub struct CheckCommand {
    #[command(flatten)]
    pub account: AccountFlag,
}

impl CheckCommand {
    pub fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        let mut config = Config::load_or_wizard(printer, config_paths)?;

        let account_name = self.account.name.as_deref();
        let Some((name, account_config)) = config.take_account(account_name)? else {
            bail!("Cannot find account");
        };

        account_config.validate()?;

        info!("checking account `{name}`");
        let sides = account_config.sides();
        if sides.is_empty() {
            bail!("Account `{name}` has no side configured (set `left` and/or `right`)");
        }
        for (side, config) in sides {
            check_side(&side.to_string(), config.clone())?;
        }

        printer.out(Message::new(format!("Account `{name}` looks healthy")))
    }
}

/// Opens the side and probes it with a `list_collections` call.
fn check_side(label: &str, config: SideConfig) -> Result<()> {
    let s = Spinner::start(format!("Checking {label} side…"));
    let mut client = client::open(config)?;
    let collections = client.list_collections(false)?;
    s.success(format!(
        "Checked {label} side ({} collections)",
        collections.len()
    ));
    Ok(())
}
