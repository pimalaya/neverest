//! # Init command
//!
//! Probes both sides and creates the pimdir replica store whose `pimdir.db`
//! marks the account initialized.

use std::{fmt, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use io_pimdir::PimdirStore;
use pimalaya_cli::{printer::Printer, spinner::Spinner};
use pimalaya_config::toml::TomlConfig;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    account::Account,
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
                "Account {name} already initialized, delete {} to reset",
                replica.display()
            );
        }

        let mode = account_config.mode()?;

        let account = Account::resolve(&account_config)?;

        let mut sources = Vec::new();

        for endpoint in account_config.endpoints()?.keys() {
            let s = Spinner::start(format!("Initializing {endpoint}…"));
            client::init(&account.get(endpoint)?)
                .with_context(|| format!("Initialize {endpoint}"))?;
            s.success(format!("Initialized {endpoint}"));
            sources.push(endpoint.clone());
        }

        sources.sort();

        let s = Spinner::start("Creating replica store…");
        fs::create_dir_all(&replica)
            .with_context(|| format!("Create replica dir {}", replica.display()))?;
        PimdirStore::open(&replica)
            .with_context(|| format!("Create pimdir store {}", replica.display()))?;
        // Stamped here, so the first sync compares against what `init`
        // opened rather than against nothing.
        StoreState::stamp(&replica, Some(&mode))
            .with_context(|| format!("Stamp store state {}", replica.display()))?;
        s.success("Created replica store");

        // Nothing can refuse a first `one-way` run, so saying what the
        // account will do stands in for a confirmation.
        printer.out(InitOutput {
            account: name,
            mode: mode.to_string(),
            store: replica,
            sources,
        })
    }
}

/// What `neverest init` reports: the store it created, and what the account
/// will do once a sync runs against it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct InitOutput {
    /// The account that was initialized.
    pub account: String,
    /// What the account will do, as its mode reads.
    pub mode: String,
    /// The replica store directory that was created.
    pub store: PathBuf,
    /// The endpoints that were opened, sorted.
    pub sources: Vec<String>,
}

impl fmt::Display for InitOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            account,
            mode,
            store,
            ..
        } = self;
        writeln!(f, "Account {account} successfully initialized: {mode}")?;
        writeln!(f, "Store created at {store}", store = store.display())
    }
}
