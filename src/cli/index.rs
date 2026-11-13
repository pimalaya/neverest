//! # Index command
//!
//! Builds a store's search index whole, the first build of a large store
//! included, where a sync refreshes it under a deadline only.

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use io_pimdir::client::{PimdirError, search::PimdirIndex};
use log::info;
use pimalaya_cli::{printer::Printer, spinner::Spinner};
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

/// Builds the search index of each store the accounts use until it holds
/// every change.
///
/// Frontends search the store through its index (index.db, pimdir SEARCH),
/// which only the store's owner builds: a sync and a drain refresh it under
/// a deadline (`store.index-timeout`), so the first build of a large store
/// spreads over several runs. This folds every change the store holds now,
/// however long it takes, reading no credential and opening no endpoint.
///
/// The index is the store's: `-a` picks the stores to build, those of the
/// accounts it names, and each is built once and whole, every account of
/// it included.
///
/// Another indexer holding an index answers `busy` with exit code 4: it
/// is building it already.
#[derive(Debug, Parser)]
pub struct IndexCommand {}

impl IndexCommand {
    /// Refreshes the index of every store the run covers until it is
    /// current, or reports it busy.
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        accounts: &[String],
    ) -> Result<Exit> {
        let config = Config::load(config_paths)?;

        let mut stores: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
        for (name, account_config) in config.select(accounts)? {
            account_config
                .validate()
                .with_context(|| format!("Validate account {name}"))?;
            let dir = driver::store_dir(&account_config)?;
            stores.entry(dir).or_default().push(name);
        }

        for (dir, accounts) in &stores {
            driver::check_store(dir)?;
            if !dir.join("pimdir.db").exists() {
                let name = &accounts[0];
                bail!("Account {name} not initialized, run `neverest init -a {name}` first");
            }
        }

        let mut built = Vec::new();
        for (dir, accounts) in stores {
            built.push(build(&dir, accounts)?);
        }

        let exit = match built.iter().any(|store| store.index.busy) {
            true => Exit::Incomplete,
            false => Exit::Success,
        };
        printer.out(IndexRunOutput { stores: built })?;

        Ok(exit)
    }
}

/// Builds the index of the store at `dir`, which `accounts` use, until it
/// is current, or until another indexer is found holding it.
fn build(dir: &Path, accounts: Vec<String>) -> Result<IndexOutput> {
    let store = dir.display();

    // NOTE: no sync.lock: the build may take long, and a sync meeting it
    // reports the index busy rather than waiting for it.
    let spinner = Spinner::start(format!("Building the search index of {store}"));
    let mut built = IndexRefresh::default();
    let mut index = match PimdirIndex::open(dir) {
        Ok(index) => index,
        Err(PimdirError::Indexing(_)) => {
            spinner.clear();
            built.busy = true;
            return Ok(IndexOutput {
                store: dir.to_path_buf(),
                accounts,
                index: built,
            });
        }
        Err(err) => {
            return Err(pimdir_error(err))
                .with_context(|| format!("Open the search index of {store}"));
        }
    };

    loop {
        let slice = index::refresh(&mut index, Some(SLICE), index::floating_zone())
            .map_err(pimdir_error)
            .with_context(|| format!("Refresh the search index of {store}"))?;
        built.folded += slice.folded;
        built.indexed += slice.indexed;
        built.behind = slice.behind;
        built.busy = slice.busy;

        if built.busy || built.behind == 0 {
            break;
        }
        spinner.set_message(format!(
            "Building the search index of {store}: {} change(s) to go",
            built.behind
        ));
    }
    spinner.clear();

    info!("search index of {store}: {built}");
    Ok(IndexOutput {
        store: dir.to_path_buf(),
        accounts,
        index: built,
    })
}

/// What `neverest index` did to the search index of every store it built.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IndexRunOutput {
    /// One entry per store, in path order.
    pub stores: Vec<IndexOutput>,
}

impl fmt::Display for IndexRunOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for store in &self.stores {
            write!(f, "{store}")?;
        }
        Ok(())
    }
}

/// What `neverest index` did to one store's search index.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IndexOutput {
    /// The store directory whose index was built.
    pub store: PathBuf,
    /// The accounts of the run using the store, in name order.
    pub accounts: Vec<String>,
    /// What the build folded, and whether another indexer held it.
    pub index: IndexRefresh,
}

impl fmt::Display for IndexOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            store,
            accounts,
            index,
        } = self;
        writeln!(
            f,
            "Search index of {} ({}): {index}",
            store.display(),
            accounts.join(", ")
        )
    }
}
