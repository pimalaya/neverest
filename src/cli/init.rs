//! `neverest init` command: probes both sides and creates the pimdir replica
//! store whose `pimdir.db` marks the account initialized.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use io_pimdir::PimdirStore;
use pimalaya_cli::{
    clap::args::AccountFlag,
    printer::{Message, Printer},
    spinner::Spinner,
};
use pimalaya_config::toml::TomlConfig;

use crate::{client, config::Config, offline::driver};

/// Initializes an account's replica; refuses to run if it already exists.
#[derive(Debug, Parser)]
pub struct InitCommand {
    #[command(flatten)]
    pub account: AccountFlag,
}

impl InitCommand {
    pub fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        let mut config = Config::load_or_wizard(printer, config_paths)?;

        let account_name = self.account.name.as_deref();
        let Some((name, account_config)) = config.take_account(account_name)? else {
            bail!("Cannot find account");
        };

        account_config.validate()?;

        let replica = driver::store_dir(&name, &account_config)?;
        let store_db = replica.join("pimdir.db");
        if store_db.exists() {
            bail!(
                "Account `{name}` already initialized, delete `{}` to reset",
                replica.display()
            );
        }

        let sides = account_config.sides();
        if sides.is_empty() {
            bail!("Account `{name}` has no side configured (set `left` and/or `right`)");
        }
        for (side, config) in sides {
            let s = Spinner::start(format!("Initializing {side} side…"));
            client::init(config.clone()).with_context(|| format!("Initialize {side} side"))?;
            s.success(format!("Initialized {side} side"));
        }

        let s = Spinner::start("Creating replica store…");
        fs::create_dir_all(&replica)
            .with_context(|| format!("Create replica dir `{}`", replica.display()))?;
        // NOTE: opening the store materializes `pimdir.db` (and its schema); the
        // handle is dropped immediately, the sync opens its own.
        PimdirStore::open(&replica, "left")
            .with_context(|| format!("Create pimdir store `{}`", replica.display()))?;
        s.success("Created replica store");

        printer.out(Message::new(format!(
            "Account `{name}` successfully initialized"
        )))
    }
}
