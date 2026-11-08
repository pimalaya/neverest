//! # Drain command
//!
//! Applies what frontends queued in the store, opening no endpoint, so
//! they read their own writes back without waiting for a sync.

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};

use anyhow::{Context, Error, Result, anyhow};
use clap::Parser;
use io_pimdir::client::producer::PimdirActionStatus;
use log::info;
use pimalaya_cli::{error::code_of, printer::Printer};
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    cli::{exit::Exit, sync::try_store_lock},
    config::{AccountConfig, Config},
    error::ErrorCode,
    offline::{driver, state::StoreState},
    sync::report::{IndexRefresh, OverruledBody},
};

/// Applies the store changes frontends queued, without syncing.
///
/// A frontend (himalaya, calendula, cardamum) queues its writes, and
/// only the store's owner applies them: until then a created item has
/// no id and no reader lists it. This applies them now, the way a sync
/// does first, reading no credential and opening no endpoint, so it
/// works offline. The next sync pushes them.
///
/// The queue is the store's: a drain applies every account's, and
/// reports each account the rows of its own collections.
///
/// Intents, a send or an invitation reply among them, need their server
/// and stay queued for the sync. A sync holding the store answers
/// `busy` with exit code 3: that run, or the next, applies them. An
/// account that could not be drained exits 3 too.
#[derive(Debug, Parser)]
pub struct DrainCommand {}

impl DrainCommand {
    /// Drains the store of every account the run covers, or reports it busy.
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        accounts: &[String],
    ) -> Result<Exit> {
        let config = Config::load_or_wizard(printer, config_paths)?;

        let mut stores: BTreeMap<PathBuf, Vec<(String, AccountConfig)>> = BTreeMap::new();
        for (name, account_config) in config.select(accounts)? {
            account_config
                .validate()
                .with_context(|| format!("Validate account {name}"))?;
            let dir = driver::store_dir(&account_config)?;
            stores.entry(dir).or_default().push((name, account_config));
        }
        for dir in stores.keys() {
            driver::refuse_former_stores(dir)?;
        }

        let order: Vec<String> = stores
            .values()
            .flatten()
            .map(|(name, _)| name.clone())
            .collect();
        let mut outputs = Vec::new();

        for (dir, accounts) in stores {
            if !dir.join("pimdir.db").exists() {
                for (name, _) in accounts {
                    let err = anyhow!(
                        "Account {name} not initialized, run `neverest init -a {name}` first"
                    );
                    outputs.push(DrainOutput::failed(name, err));
                }
                continue;
            }

            // NOTE: never waits: a sync holding the store drains it itself,
            // at its start or at the next run.
            let lock = try_store_lock(&dir)
                .with_context(|| format!("Acquire the lock of the store at {}", dir.display()))?;
            let Some(_lock) = lock else {
                outputs.extend(
                    accounts
                        .into_iter()
                        .map(|(name, _)| DrainOutput::busy(name)),
                );
                continue;
            };
            driver::check_store(&dir)?;

            outputs.extend(drain(&dir, accounts)?);
        }

        outputs.sort_by_key(|output| order.iter().position(|name| *name == output.account));
        let exit = match outputs.iter().any(|o| o.busy || o.error.is_some()) {
            true => Exit::Incomplete,
            false => Exit::Success,
        };

        printer.out(DrainRunOutput { accounts: outputs })?;
        Ok(exit)
    }
}

/// Declares the sources of `accounts` as a sync does before its drain, then
/// drains the store at `dir` once, telling each account the rows of its own
/// collections. An account that cannot be declared fails alone.
fn drain(dir: &Path, accounts: Vec<(String, AccountConfig)>) -> Result<Vec<DrainOutput>> {
    let mut outputs = Vec::new();
    let mut ready = Vec::new();

    // NOTE: from the configuration and the calendars the last listing
    // stated take no tasks (pimdir STORAGE §15.6), as a sync does.
    for (name, account_config) in accounts {
        let declared = account_config.namespaced(&name).and_then(|namespaced| {
            let mode = namespaced.mode()?;
            let state = StoreState::load(dir)?.account(&name)?;
            driver::declare(dir, &name, &namespaced, &mode, &state)?;
            Ok(namespaced)
        });
        match declared {
            Ok(namespaced) => ready.push((name, namespaced)),
            Err(err) => outputs.push(DrainOutput::failed(name, err)),
        }
    }

    let Some((first_name, first_config)) = ready.first() else {
        return Ok(outputs);
    };
    let mode = first_config.mode()?;
    let first = mode
        .sources
        .first()
        .expect("a validated account has at least one source");
    let mut store = driver::open_store(dir, first, first_name)?;
    let queued = store
        .list_pending_actions()
        .context("List the queued actions")?;

    let mut drained = driver::drain_store(&mut store);

    // NOTE: briefly, a frontend waiting on the drain; once, the index
    // being the store's.
    #[cfg_attr(not(feature = "search"), allow(unused_mut))]
    let mut index = None;
    #[cfg(feature = "search")]
    crate::offline::index::after_run(dir, &first_config.store, true, &mut index);

    let mut mine: BTreeMap<String, DrainOutput> = ready
        .iter()
        .map(|(name, _)| {
            let mut output = DrainOutput::new(name.clone());
            let report = drained.remove(name).unwrap_or_default();
            output.overruled = report.overruled;
            output.index = index.clone();
            (name.clone(), output)
        })
        .collect();

    let owners = driver::collection_accounts(&store);
    for row in queued {
        let Some(output) = owners.get(&row.collection).and_then(|o| mine.get_mut(o)) else {
            continue;
        };
        let status = store
            .action_status(row.id)
            .with_context(|| format!("Read where queued action {} stands", row.id))?;
        let kind = row.action.kind().to_owned();
        match status {
            PimdirActionStatus::Applied { seq, .. } => output.applied.push(DrainedAction {
                id: row.id,
                collection: row.collection,
                kind,
                seq,
            }),
            PimdirActionStatus::Parked { error, .. } => output.parked.push(ParkedAction {
                id: row.id,
                collection: row.collection,
                kind,
                error,
            }),
            PimdirActionStatus::Pending { .. } => output.waiting += 1,
            PimdirActionStatus::Unknown => {}
        }
    }

    for output in mine.values() {
        info!(
            "drained {} queued action(s) of {} ({} parked, {} waiting)",
            output.applied.len(),
            output.account,
            output.parked.len(),
            output.waiting,
        );
    }

    outputs.extend(mine.into_values());
    Ok(outputs)
}

/// What `neverest drain` did with the queue of every account it covered.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DrainRunOutput {
    /// One entry per account the run covered.
    pub accounts: Vec<DrainOutput>,
}

impl fmt::Display for DrainRunOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for account in &self.accounts {
            write!(f, "{account}")?;
        }
        Ok(())
    }
}

/// What `neverest drain` did with one account's queue.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DrainOutput {
    /// The account whose store was drained.
    pub account: String,
    /// A sync held the store, so nothing was drained.
    pub busy: bool,
    /// The stable code of the failure that stopped the account, `null`
    /// when it was drained or the failure carries none.
    pub code: Option<ErrorCode>,
    /// Why the account was not drained, `null` when it was.
    pub error: Option<String>,
    /// The actions applied to the account's collections, in queue order.
    pub applied: Vec<DrainedAction>,
    /// The actions refused for good this time, kept for an operator.
    pub parked: Vec<ParkedAction>,
    /// The rows left queued for a sync: intents, a send or a reply, and
    /// actions a transient failure deferred.
    pub waiting: usize,
    /// The bodies a collection's conflict policy kept aside as retained
    /// items while an action was applied (pimdir SYNC §9).
    pub overruled: Vec<OverruledBody>,
    /// What the drain did to the store's search index, `null` in a
    /// build without the `search` feature: a failure is reported here and
    /// never fails the drain. The index is the store's, so every account
    /// of one store reports the same refresh.
    pub index: Option<IndexRefresh>,
}

impl DrainOutput {
    fn new(account: String) -> Self {
        Self {
            account,
            busy: false,
            code: None,
            error: None,
            applied: Vec::new(),
            parked: Vec::new(),
            waiting: 0,
            overruled: Vec::new(),
            index: None,
        }
    }

    fn busy(account: String) -> Self {
        Self {
            busy: true,
            ..Self::new(account)
        }
    }

    fn failed(account: String, err: Error) -> Self {
        Self {
            code: code_of(&err),
            error: Some(format!("{err:#}")),
            ..Self::new(account)
        }
    }
}

impl fmt::Display for DrainOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.busy {
            return writeln!(
                f,
                "A sync holds the store of account {}: it applies the queue itself",
                self.account
            );
        }

        if let Some(error) = &self.error {
            return writeln!(f, "Account {} was not drained: {error}", self.account);
        }

        writeln!(
            f,
            "Applied {} queued action(s) of account {}",
            self.applied.len(),
            self.account
        )?;
        for action in &self.applied {
            writeln!(f, "  {action}")?;
        }
        for action in &self.parked {
            writeln!(f, "  {action}")?;
        }
        for overruled in &self.overruled {
            writeln!(f, "  {overruled}")?;
        }
        if self.waiting > 0 {
            writeln!(f, "{} left queued for the next sync", self.waiting)?;
        }
        if let Some(index) = &self.index {
            writeln!(f, "Search index: {index}")?;
        }
        Ok(())
    }
}

/// One queue row the drain applied.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DrainedAction {
    /// The queue row, as its producer's enqueue returned it.
    pub id: i64,
    /// The collection it was queued on.
    pub collection: String,
    /// The action kind: `add`, `update`, `set-flags`, `remove`, `move`,
    /// `copy`.
    pub kind: String,
    /// The item an `add` created, the id readers list it under.
    pub seq: Option<i64>,
}

impl fmt::Display for DrainedAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{} {} in {}", self.id, self.kind, self.collection)?;
        if let Some(seq) = self.seq {
            write!(f, " (item {seq})")?;
        }
        Ok(())
    }
}

/// One queue row the drain parked.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParkedAction {
    /// The queue row.
    pub id: i64,
    /// The collection it was queued on.
    pub collection: String,
    /// The action kind.
    pub kind: String,
    /// Why the store refused it.
    pub error: String,
}

impl fmt::Display for ParkedAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "#{} {} in {} parked: {}",
            self.id, self.kind, self.collection, self.error
        )
    }
}
