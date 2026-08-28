//! `neverest check` command: reports what the store keeps, then opens every
//! source and lists its collections to surface credential, network or config
//! errors before a real sync.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Parser;
use log::info;
use pimalaya_cli::{
    printer::{Message, Printer},
    spinner::Spinner,
};
use pimalaya_config::toml::TomlConfig;

use crate::{account::Account, client, config::Config};

/// Reports the account's namespaces, then opens every configured source and
/// lists its collections, surfacing credential, network or config errors
/// before a real sync.
#[derive(Debug, Parser)]
pub struct CheckCommand {}

impl CheckCommand {
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        account_name: Option<&str>,
    ) -> Result<()> {
        let mut config = Config::load_or_wizard(printer, config_paths)?;

        let Some((name, account_config)) = config.take_account(account_name)? else {
            bail!("Cannot find account");
        };

        account_config.validate()?;

        info!("checking account {name}");

        printer.out(Message::new(account_config.mode()?.to_string()))?;

        // Every credential at once, so a check costs one unlock per
        // distinct password command rather than one per endpoint.
        let account = Account::resolve(&account_config)?;

        for endpoint in account_config.endpoints()?.keys() {
            check_source(endpoint, &account)?;
        }

        printer.out(Message::new(format!("Account {name} looks healthy")))
    }
}

/// Opens the source and probes it with a `list_collections` call.
fn check_source(label: &str, account: &Account) -> Result<()> {
    let s = Spinner::start(format!("Checking source {label}…"));
    let mut client = client::open(&account.get(label)?)?;
    let collections = client.list_collections(false)?;
    s.success(format!(
        "Checked source {label} ({} collections)",
        collections.len()
    ));
    Ok(())
}
