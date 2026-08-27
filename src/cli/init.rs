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

use crate::{
    client,
    config::Config,
    offline::{driver, state::StoreState},
};

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

        let mode = account_config.mode()?;

        for (endpoint, config) in account_config.endpoints()? {
            let s = Spinner::start(format!("Initializing {endpoint}…"));
            client::init(config).with_context(|| format!("Initialize {endpoint}"))?;
            s.success(format!("Initialized {endpoint}"));
        }

        let s = Spinner::start("Creating replica store…");
        fs::create_dir_all(&replica)
            .with_context(|| format!("Create replica dir `{}`", replica.display()))?;
        PimdirStore::open(&replica)
            .with_context(|| format!("Create pimdir store `{}`", replica.display()))?;
        // The mode is stamped here, so the first sync compares against what
        // `init` opened rather than against nothing.
        StoreState::stamp(&replica, Some(&mode))
            .with_context(|| format!("Stamp store state `{}`", replica.display()))?;
        s.success("Created replica store");

        // A first run under `one-way` has no recorded mode to compare against,
        // so nothing can refuse it. Saying what the account will do is what
        // stands in for the confirmation a one-shot tool cannot ask for.
        printer.out(Message::new(format!(
            "Account `{name}` successfully initialized: {mode}"
        )))
    }
}
