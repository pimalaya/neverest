//! # Index command
//!
//! Builds the store's search index whole, the first build of a large store
//! included, where a sync refreshes it under a deadline only.

use std::{fmt, path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use clap::Parser;
use io_pimdir::client::{PimdirError, search::PimdirIndex};
use log::info;
use pimalaya_cli::{printer::Printer, spinner::Spinner};
use pimalaya_config::toml::TomlConfig;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    cli::exit::Exit,
    config::Config,
    error::pimdir_error,
    offline::{driver, index},
    sync::report::IndexRefresh,
};

/// How long one slice of the build runs before it says how far it got.
const SLICE: Duration = Duration::from_secs(1);

/// Builds the store's search index until it holds every change.
///
/// Frontends search the store through its index (index.db, pimdir SEARCH),
/// which only the store's owner builds: a sync and a drain refresh it under
/// a deadline (`store.index-timeout`), so the first build of a large store
/// spreads over several runs. This folds every change the store holds now,
/// however long it takes, reading no credential and opening no endpoint.
///
/// Another indexer holding the index answers `busy` with exit code 3: it
/// is building it already.
#[derive(Debug, Parser)]
pub struct IndexCommand {}

impl IndexCommand {
    /// Refreshes the account's index until it is current, or reports it
    /// busy.
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        account_name: Option<&str>,
    ) -> Result<Exit> {
        let mut config = Config::load_or_wizard(printer, config_paths)?;

        let Some((name, account_config)) = config.take_account(account_name)? else {
            bail!("Cannot find account");
        };

        account_config.validate()?;

        let dir = driver::store_dir(&name, &account_config)?;
        if !dir.join("pimdir.db").exists() {
            bail!("Account {name} not initialized, run `init -a {name}` first");
        }

        // NOTE: no sync.lock: the build may take long, and a sync meeting
        // it reports the index busy rather than waiting for it.
        let spinner = Spinner::start(format!("Building the search index of account {name}"));
        let mut built = IndexRefresh::default();
        let mut index = match PimdirIndex::open(&dir) {
            Ok(index) => index,
            Err(PimdirError::Indexing(_)) => {
                spinner.clear();
                built.busy = true;
                printer.out(IndexOutput {
                    account: name,
                    index: built,
                })?;
                return Ok(Exit::Incomplete);
            }
            Err(err) => {
                return Err(pimdir_error(err))
                    .with_context(|| format!("Open the search index of account {name}"));
            }
        };

        loop {
            let slice = index::refresh(&mut index, Some(SLICE))
                .map_err(pimdir_error)
                .with_context(|| format!("Refresh the search index of account {name}"))?;
            built.folded += slice.folded;
            built.indexed += slice.indexed;
            built.behind = slice.behind;
            built.busy = slice.busy;

            if built.busy || built.behind == 0 {
                break;
            }
            spinner.set_message(format!(
                "Building the search index of account {name}: {} change(s) to go",
                built.behind
            ));
        }
        spinner.clear();

        info!("search index of account {name}: {built}");
        let exit = match built.busy {
            true => Exit::Incomplete,
            false => Exit::Success,
        };
        printer.out(IndexOutput {
            account: name,
            index: built,
        })?;

        Ok(exit)
    }
}

/// What `neverest index` did to the account's search index.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IndexOutput {
    /// The account whose index was built.
    pub account: String,
    /// What the build folded, and whether another indexer held it.
    pub index: IndexRefresh,
}

impl fmt::Display for IndexOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { account, index } = self;
        writeln!(f, "Search index of account {account}: {index}")
    }
}
