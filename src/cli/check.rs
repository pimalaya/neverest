//! `neverest check` command.
//!
//! Opens both sides and asks each one to `list_mailboxes`. The
//! operation itself is cheap; the value is in surfacing the
//! credential / network / config errors that would otherwise only
//! show up during a real sync.

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
    config::{Config, SideConfig},
    side::Side,
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
        check_side("left", account_config.left, Side::Left)?;
        check_side("right", account_config.right, Side::Right)?;

        printer.out(Message::new(format!("Account `{name}` looks healthy")))
    }
}

fn check_side(label: &str, side_config: SideConfig, side: Side) -> Result<()> {
    let s = Spinner::start(format!("Checking {label} side…"));
    info!("checking {label} side: opening client");
    let mut client = side.open(side_config)?;
    let mailboxes = client.list_mailboxes(false)?;
    info!("checking {label} side: OK ({} mailboxes)", mailboxes.len());
    s.success(format!(
        "Checked {label} side ({} mailboxes)",
        mailboxes.len()
    ));
    Ok(())
}
