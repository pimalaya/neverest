//! # Index-only sync
//!
//! `sync --index-only`: builds a store's search index whole, the first
//! build of a large store included, where a sync refreshes it under a
//! deadline only.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use io_pimdir::client::{PimdirError, search::PimdirIndex};
use log::info;
use pimalaya_cli::printer::Printer;

use crate::{
    cli::exit::Exit,
    config::Config,
    error::pimdir_error,
    sync::{
        driver, index,
        report::{IndexOutput, IndexRefresh, SyncRunOutput},
    },
    zone::floating_zone,
};

/// How long one slice of the build runs before it says how far it got.
const SLICE: Duration = Duration::from_secs(1);

/// Refreshes the index of every store the run covers until it is current,
/// or reports it busy.
pub fn run(
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
            bail!(
                "Account {name} not initialized, run `neverest init -a {name}` or \
                 `neverest sync --init -a {name}` first"
            );
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
    printer.out(SyncRunOutput {
        index: built,
        ..SyncRunOutput::default()
    })?;

    Ok(exit)
}

/// Builds the index of the store at `dir`, which `accounts` use, until it
/// is current, or until another indexer is found holding it.
///
/// Takes no sync.lock: the build may take long, and a sync meeting it
/// reports the index busy rather than waiting for it.
fn build(dir: &Path, accounts: Vec<String>) -> Result<IndexOutput> {
    let store = dir.display();

    crate::notice::print(format!("Building the search index of {store}"));
    let mut built = IndexRefresh::default();
    let mut index = match PimdirIndex::open(dir) {
        Ok(index) => index,
        Err(PimdirError::Indexing(_)) => {
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
        let slice = index::refresh(&mut index, Some(SLICE), floating_zone())
            .map_err(pimdir_error)
            .with_context(|| format!("Refresh the search index of {store}"))?;
        built.folded += slice.folded;
        built.indexed += slice.indexed;
        built.behind = slice.behind;
        built.busy = slice.busy;

        if built.busy || built.behind == 0 {
            break;
        }
    }

    info!("search index of {store}: {built}");
    Ok(IndexOutput {
        store: dir.to_path_buf(),
        accounts,
        index: built,
    })
}
