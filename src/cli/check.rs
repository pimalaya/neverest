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
