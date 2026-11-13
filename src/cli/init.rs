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
use pimalaya_cli::{error::code_of, printer::Printer, spinner::Spinner};
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    account::Account,
    cli::{
        exit::Exit,
        sync::{LOCK_TIMEOUT, wait_store_lock},
    },
    client,
    config::{AccountConfig, AccountMode, Config},
    error::ErrorCode,
    offline::{driver, state::StoreState},
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
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        accounts: &[String],
    ) -> Result<Exit> {
        let config = Config::load(config_paths)?;

        let mut stores: BTreeMap<PathBuf, Vec<(String, AccountConfig)>> = BTreeMap::new();
        for (name, account_config) in config.select(accounts)? {
            account_config
                .validate()
                .with_context(|| format!("Validate account {name}"))?;
            let dir = driver::store_dir(&account_config)?;
            stores.entry(dir).or_default().push((name, account_config));
        }
        // NOTE: before anything is created: a directory holding the stores
        // an earlier neverest kept per account is refused as it is.
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
            // NOTE: io-pimdir restricts the directory to its user when it
            // creates the store in it (pimdir STORAGE §18).
            fs::create_dir_all(&dir)
                .with_context(|| format!("Create store dir {}", dir.display()))?;
            let lock = wait_store_lock(&dir, LOCK_TIMEOUT)
                .with_context(|| format!("Acquire the lock of the store at {}", dir.display()))?;
            let Some(_lock) = lock else {
                for (name, account_config) in accounts {
                    let err = anyhow!(
                        "Another run holds the store at {} after {}s",
                        dir.display(),
                        LOCK_TIMEOUT.as_secs()
                    );
                    outputs.push(InitOutput::failed(name, account_config.mode()?, &dir, err));
                }
                continue;
            };

            for (name, account_config) in accounts {
                let output = match init(&dir, &name, &account_config) {
                    Ok(output) => output,
                    Err(err) => InitOutput::failed(name, account_config.mode()?, &dir, err),
                };
                outputs.push(output);
            }
        }

        outputs.sort_by_key(|output| order.iter().position(|name| *name == output.account));
        let exit = match outputs.iter().any(|output| output.error.is_some()) {
            true => Exit::Incomplete,
            false => Exit::Success,
        };

        // NOTE: nothing can refuse a first `one-way` run, so saying what each
        // account will do stands in for a confirmation.
        printer.out(InitRunOutput { accounts: outputs })?;
        Ok(exit)
    }
}

/// Opens every endpoint of `account`, then adds it to the store at `dir`,
/// creating the store when there is none. An account initialized already
/// is left as it is: its store may hold work no server has.
fn init(dir: &Path, name: &str, account_config: &AccountConfig) -> Result<InitOutput> {
    let mode = account_config.mode()?;

    if StoreState::load(dir)?.initialized(name) {
        return Ok(InitOutput {
            account: name.to_owned(),
            mode,
            store: dir.to_path_buf(),
            sources: Vec::new(),
            already_initialized: true,
            code: None,
            error: None,
        });
    }

    let account = Account::resolve(account_config)?;
    let mut sources = Vec::new();
    for endpoint in account_config.endpoints()?.keys() {
        let s = Spinner::start(format!("Initializing {endpoint} of account {name}…"));
        client::init(&account.get(endpoint)?).with_context(|| format!("Initialize {endpoint}"))?;
        s.success(format!("Initialized {endpoint} of account {name}"));
        sources.push(endpoint.clone());
    }
    sources.sort();

    if !dir.join("pimdir.db").exists() {
        let s = Spinner::start("Creating the store…");
        driver::create_store(dir)?;
        s.success("Created the store");
    }
    // NOTE: stamped here, so the first sync compares against what `init`
    // opened rather than against nothing.
    StoreState::stamp(dir, name, &mode)
        .with_context(|| format!("Stamp store state {}", dir.display()))?;

    Ok(InitOutput {
        account: name.to_owned(),
        mode,
        store: dir.to_path_buf(),
        sources,
        already_initialized: false,
        code: None,
        error: None,
    })
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
    /// The endpoints that were opened, sorted; empty when the account was
    /// initialized already or failed.
    pub sources: Vec<String>,
    /// Whether the account was initialized already, and left as it was.
    pub already_initialized: bool,
    /// The stable code of the failure that stopped the account, `null`
    /// when it was initialized or the failure carries none.
    pub code: Option<ErrorCode>,
    /// Why the account was not initialized, `null` when it was.
    pub error: Option<String>,
}

impl InitOutput {
    fn failed(account: String, mode: AccountMode, store: &Path, err: Error) -> Self {
        Self {
            account,
            mode,
            store: store.to_path_buf(),
            sources: Vec::new(),
            already_initialized: false,
            code: code_of(&err),
            error: Some(format!("{err:#}")),
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
