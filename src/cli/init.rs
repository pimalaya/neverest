//! `neverest init` command: probes both sides and creates the pimdir replica
//! store whose `pimdir.db` marks the account initialized.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use io_pimdir::PimdirStore;
use pimalaya_cli::{
    printer::{Message, Printer},
    spinner::Spinner,
};
use pimalaya_config::toml::TomlConfig;

use crate::{client, config::Config, offline::driver};

/// Initializes an account's replica; refuses to run if it already exists.
#[derive(Debug, Parser)]
pub struct InitCommand {}

impl InitCommand {
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

        let replica = driver::store_dir(&name, &account_config)?;
        let store_db = replica.join("pimdir.db");
        if store_db.exists() {
            bail!(
                "Account `{name}` already initialized, delete `{}` to reset",
                replica.display()
            );
        }

        for (source, config) in account_config.sources()? {
            let s = Spinner::start(format!("Initializing source {source}…"));
            client::init(config).with_context(|| format!("Initialize source {source}"))?;
            s.success(format!("Initialized source {source}"));
        }

        let s = Spinner::start("Creating replica store…");
        fs::create_dir_all(&replica)
            .with_context(|| format!("Create replica dir `{}`", replica.display()))?;
        // NOTE: opening the store materializes `pimdir.db` and its schema. The
        // handle is dropped immediately, the sync opening its own.
        PimdirStore::open(&replica)
            .with_context(|| format!("Create pimdir store `{}`", replica.display()))?;
        s.success("Created replica store");

        printer.out(Message::new(format!(
            "Account `{name}` successfully initialized"
        )))
    }
}
