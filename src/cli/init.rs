//! # Init command
//!
//! Probes every endpoint of each account the run covers, then adds the
//! account to the pimdir store it shares, creating the store when there is
//! none. The account's sidecar entry marks it initialized.

use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Error, Result, anyhow};
use clap::Parser;
use pimalaya_cli::printer::Printer;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    account::Account,
    cli::{
        exit::Exit,
        sync::{LOCK_TIMEOUT, wait_store_lock},
    },
    client,
    config::{AccountConfig, Config, mode::AccountMode},
    error::{ErrorCode, Failure, chain},
    sync::{driver, state::StoreState},
};

/// Initializes every account, or those `-a` names, in the store it shares.
///
/// Opens each endpoint first, so credential and network errors surface up
/// front, then adds the account to the store, creating the store when there
/// is none. An account already initialized is left as it is and said so; one
/// that could not be initialized is reported, the others still are, and the
/// command exits 4.
#[derive(Debug, Parser)]
pub struct InitCommand {}

impl InitCommand {
    /// Initializes the accounts the run covers, store by store.
    ///
    /// Nothing can refuse a first `one-way` run, so the report of what each
    /// account will do stands in for a confirmation.
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        accounts: &[String],
    ) -> Result<Exit> {
        let config = Config::load(config_paths)?;
        let outputs = initialize(config.select(accounts)?)?;
        let exit = match outputs.iter().any(|output| output.error.is_some()) {
            true => Exit::Incomplete,
            false => Exit::Success,
        };

        printer.out(InitRunOutput { accounts: outputs })?;
        Ok(exit)
    }
}

/// Initializes `selected` store by store, in the order `-a` and the
/// configuration give; what `init` and `sync --init` run.
///
/// One that could not be initialized is reported, the others still are. A
/// directory holding the per-account stores of an earlier neverest is
/// refused before anything is created.
pub fn initialize(selected: Vec<(String, AccountConfig)>) -> Result<Vec<InitOutput>> {
    let mut stores: BTreeMap<PathBuf, Vec<(String, AccountConfig)>> = BTreeMap::new();
    for (name, account_config) in selected {
        account_config
            .validate()
            .with_context(|| format!("Validate account {name}"))?;
        let dir = driver::store_dir(&account_config)?;
        stores.entry(dir).or_default().push((name, account_config));
    }
    for dir in stores.keys() {
        driver::check_store(dir)?;
    }

    let order: Vec<String> = stores
        .values()
        .flatten()
        .map(|(name, _)| name.clone())
        .collect();
    let mut outputs = Vec::new();

    for (dir, accounts) in stores {
        fs::create_dir_all(&dir).with_context(|| format!("Create store dir {}", dir.display()))?;
        let lock = wait_store_lock(&dir, LOCK_TIMEOUT)
            .with_context(|| format!("Acquire the lock of the store at {}", dir.display()))?;
        let Some(_lock) = lock else {
            for (name, account_config) in accounts {
                let err = anyhow!(
                    "Another run holds the store at {} after {}s",
                    dir.display(),
                    LOCK_TIMEOUT.as_secs()
                );
                let mode = account_config.mode()?;
                outputs.push(InitOutput::failed(name, mode, &dir, &[], err));
            }
            continue;
        };

        for (name, account_config) in accounts {
            let mut opened = Vec::new();
            let output = match init(&dir, &name, &account_config, &mut opened) {
                Ok(output) => output,
                Err(err) => {
                    let mode = account_config.mode()?;
                    InitOutput::failed(name, mode, &dir, &opened, err)
                }
            };
            outputs.push(output);
        }
    }

    outputs.sort_by_key(|output| order.iter().position(|name| *name == output.account));
    Ok(outputs)
}

/// Opens every endpoint of `account`, then adds it to the store at `dir`,
/// creating the store when there is none.
///
/// An initialized account is left as it is: its store may hold work no
/// server has. `opened` names each endpoint reached, even on failure. The
/// state is stamped so the first sync compares against what `init` opened.
fn init(
    dir: &Path,
    name: &str,
    account_config: &AccountConfig,
    opened: &mut Vec<String>,
) -> Result<InitOutput> {
    let mode = account_config.mode()?;

    if StoreState::load(dir)?.initialized(name) {
        return Ok(InitOutput {
            account: name.to_owned(),
            mode,
            store: dir.to_path_buf(),
            opened: Vec::new(),
            already_initialized: true,
            code: None,
            error: None,
            sources: Vec::new(),
        });
    }

    let account = Account::resolve(account_config)?;
    for endpoint in account_config.endpoints()?.keys() {
        client::init(&account.get(endpoint)?).with_context(|| format!("Initialize {endpoint}"))?;
        crate::notice::print(format!("Initialized {endpoint} of account {name}"));
        opened.push(endpoint.clone());
    }

    if !dir.join("pimdir.db").exists() {
        driver::create_store(dir)?;
        crate::notice::print("Created the store");
    }
    StoreState::stamp(dir, name, &mode)
        .with_context(|| format!("Stamp store state {}", dir.display()))?;

    Ok(InitOutput {
        account: name.to_owned(),
        mode,
        store: dir.to_path_buf(),
        opened: sorted(opened),
        already_initialized: false,
        code: None,
        error: None,
        sources: Vec::new(),
    })
}

/// The endpoints `init` opened, sorted, so a report reads the same twice.
fn sorted(opened: &[String]) -> Vec<String> {
    let mut opened = opened.to_vec();
    opened.sort();
    opened
}

/// What `neverest init` reports of every account it covered.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InitRunOutput {
    /// One entry per account the run covered.
    pub accounts: Vec<InitOutput>,
}

impl fmt::Display for InitRunOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for account in &self.accounts {
            write!(f, "{account}")?;
        }
        Ok(())
    }
}

/// What `neverest init` reports of one account: the store it joined, and
/// what the account will do once a sync runs against it.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InitOutput {
    /// The account.
    pub account: String,
    /// What the account will do: its endpoints, direction and whether the
    /// store keeps bodies.
    pub mode: AccountMode,
    /// The store directory the account syncs into, which every account
    /// naming no `store.root` shares.
    pub store: PathBuf,
    /// The endpoints `init` opened, sorted, those it opened before a
    /// failure included; empty when the account was initialized already.
    pub opened: Vec<String>,
    /// Whether the account was initialized already, and left as it was.
    pub already_initialized: bool,
    /// The stable code of the failure that stopped the account, `null`
    /// when it was initialized or the failure carries none.
    pub code: Option<ErrorCode>,
    /// Why the account was not initialized, the outermost message, `null`
    /// when it was.
    pub error: Option<String>,
    /// The messages of the failure's causes, outermost first, empty when
    /// the account was initialized.
    pub sources: Vec<String>,
}

impl InitOutput {
    fn failed(
        account: String,
        mode: AccountMode,
        store: &Path,
        opened: &[String],
        err: Error,
    ) -> Self {
        let failure = Failure::of(&err);
        Self {
            account,
            mode,
            store: store.to_path_buf(),
            opened: sorted(opened),
            already_initialized: false,
            code: failure.code,
            error: Some(failure.error),
            sources: failure.sources,
        }
    }
}

impl fmt::Display for InitOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            account,
            mode,
            store,
            ..
        } = self;
        let store = store.display();

        if let Some(error) = &self.error {
            let error = chain(error, &self.sources);
            return writeln!(f, "Account {account} was not initialized: {error}");
        }
        if self.already_initialized {
            return writeln!(
                f,
                "Account {account} already initialized in {store}: {mode}"
            );
        }

        writeln!(f, "Account {account} successfully initialized: {mode}")?;
        writeln!(f, "Store at {store}")
    }
}
