//! # Index-only sync
//!
//! `sync --index-only`: builds a store's search index whole, the first
//! build of a large store included, where a sync refreshes it under a
//! deadline only.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use io_pimdir::client::{PimdirError, search::PimdirIndex};
use log::info;
use pimalaya_cli::printer::Printer;

use crate::{
    cli::{exit::Exit, sync},
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
    let stores = driver::group_stores(config.select(accounts)?)?;

    let mut failed = Vec::new();
    let mut ready = Vec::new();
    for (dir, accounts) in stores {
        let names: Vec<String> = accounts.into_iter().map(|(name, _)| name).collect();
        if !dir.join("pimdir.db").exists() {
            for name in names {
                let err = driver::not_initialized(&name);
                failed.push(sync::failed(name, err));
            }
            continue;
        }
        driver::check_store(&dir)?;
        ready.push((dir, names));
    }

    let mut built = Vec::new();
    for (dir, accounts) in ready {
        built.push(build(&dir, accounts)?);
    }

    let busy = built.iter().any(|store| store.index.busy);
    let exit = match busy || !failed.is_empty() {
        true => Exit::Incomplete,
        false => Exit::Success,
    };
    printer.out(SyncRunOutput {
        accounts: failed,
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
