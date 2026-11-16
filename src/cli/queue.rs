//! # Queue-only sync
//!
//! `sync --queue-only`: applies what frontends queued in the store, opening
//! no endpoint, so they read their own writes back without waiting for a
//! full sync.

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};

use anyhow::{Context, Error, Result, anyhow};
use io_pimdir::client::producer::PimdirActionStatus;
use log::info;
use pimalaya_cli::printer::Printer;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    cli::{exit::Exit, sync::try_store_lock},
    config::{AccountConfig, Config},
    error::{ErrorCode, Failure, chain},
    sync::{
        driver,
        report::{IndexRefresh, OverruledBody, SyncRunOutput},
        state::StoreState,
    },
};

/// Drains the store of every account the run covers, or reports it busy.
///
/// Never waits for the lock: a sync holding the store drains it itself, at
/// its start or at the next run.
pub fn run(
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
                    "Account {name} not initialized, run `neverest init -a {name}` or \
                     `neverest sync --init -a {name}` first"
                );
                outputs.push(QueueOutput::failed(name, err));
            }
            continue;
        }

        let lock = try_store_lock(&dir)
            .with_context(|| format!("Acquire the lock of the store at {}", dir.display()))?;
        let Some(_lock) = lock else {
            outputs.extend(
                accounts
                    .into_iter()
                    .map(|(name, _)| QueueOutput::busy(name)),
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

    printer.out(SyncRunOutput {
        queue: outputs,
        ..SyncRunOutput::default()
    })?;
    Ok(exit)
}

/// Declares the sources of `accounts` as a sync does, then drains the store
/// at `dir` once; an account that cannot be declared fails alone.
///
/// Declaring reads the configuration and the calendars the last listing
/// stated take no tasks (pimdir STORAGE §15.6). A drain performs no intent,
/// so no row it reports is unconfirmed. The index refreshes once, briefly.
fn drain(dir: &Path, accounts: Vec<(String, AccountConfig)>) -> Result<Vec<QueueOutput>> {
    let mut outputs = Vec::new();
    let mut ready = Vec::new();

    for (name, account_config) in accounts {
        let declared = account_config.namespaced(&name).and_then(|namespaced| {
            let mode = namespaced.mode()?;
            let state = StoreState::load(dir)?.account(&name)?;
            driver::declare(dir, &name, &namespaced, &mode, &state)?;
            Ok(namespaced)
        });
        match declared {
            Ok(namespaced) => ready.push((name, namespaced)),
            Err(err) => outputs.push(QueueOutput::failed(name, err)),
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

    #[cfg_attr(not(feature = "search"), allow(unused_mut))]
    let mut index = None;
    #[cfg(feature = "search")]
    crate::sync::index::after_run(dir, &first_config.store, true, &mut index);

    let mut mine: BTreeMap<String, QueueOutput> = ready
        .iter()
        .map(|(name, _)| {
            let mut output = QueueOutput::new(name.clone());
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
            PimdirActionStatus::Parked {
                attempts, error, ..
            } => output.parked.push(ParkedAction {
                id: row.id,
                collection: row.collection,
                kind,
                producer: row.producer,
                queued_at: row.created_at,
                attempts,
                code: None,
                error,
                sources: Vec::new(),
            }),
            PimdirActionStatus::Pending { .. } => output.waiting += 1,
            PimdirActionStatus::Unconfirmed { .. }
            | PimdirActionStatus::Reset
            | PimdirActionStatus::Unknown => {}
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

/// What `neverest sync --queue-only` did with one account's queue.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueueOutput {
    /// The account whose store was drained.
    pub account: String,
    /// A sync held the store, so nothing was drained.
    pub busy: bool,
    /// The stable code of the failure that stopped the account, `null`
    /// when it was drained or the failure carries none.
    pub code: Option<ErrorCode>,
    /// Why the account was not drained, the outermost message, `null` when
    /// it was.
    pub error: Option<String>,
    /// The messages of the failure's causes, outermost first, empty when it
    /// was drained.
    pub sources: Vec<String>,
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

impl QueueOutput {
    fn new(account: String) -> Self {
        Self {
            account,
            busy: false,
            code: None,
            error: None,
            sources: Vec::new(),
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
        let failure = Failure::of(&err);
        Self {
            code: failure.code,
            error: Some(failure.error),
            sources: failure.sources,
            ..Self::new(account)
        }
    }
}

impl fmt::Display for QueueOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.busy {
            return writeln!(
                f,
                "A sync holds the store of account {}: it applies the queue itself",
                self.account
            );
        }

        if let Some(error) = &self.error {
            let error = chain(error, &self.sources);
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
    #[serde(rename = "row")]
    pub id: i64,
    /// The collection it was queued on.
    pub collection: String,
    /// The action kind, kebab-case as the store spells it: `add`, `update`,
    /// `set-flags`, `remove`, `move`, `copy`.
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
    #[serde(rename = "row")]
    pub id: i64,
    /// The collection it was queued on.
    pub collection: String,
    /// The action kind, kebab-case as the store spells it: `add`, `update`,
    /// `set-flags`, `remove`, `move`, `copy`.
    pub kind: String,
    /// The enqueuing process, diagnostic only.
    pub producer: String,
    /// When the row was appended, stamped by the store's own clock.
    pub queued_at: String,
    /// How many times the row was tried before it was parked.
    pub attempts: i64,
    /// The stable code of the refusal, `null`: the store keeps its text
    /// alone.
    pub code: Option<ErrorCode>,
    /// Why the store refused it, as the store recorded it.
    pub error: String,
    /// The messages of the refusal's causes, empty: the store recorded
    /// them in `error`.
    pub sources: Vec<String>,
}

impl fmt::Display for ParkedAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "#{} {} in {} parked: {}",
            self.id,
            self.kind,
            self.collection,
            chain(&self.error, &self.sources)
        )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cli::queue::*;

    /// A drained or parked row is keyed `row`, as himalaya's `pimdir queue
    /// list` keys it.
    ///
    /// A parked one carries its producer, when it was queued and how often
    /// it was tried.
    #[test]
    fn a_drain_keys_its_rows_row() {
        let mut output = QueueOutput::new(String::from("work"));
        output.applied.push(DrainedAction {
            id: 2,
            collection: String::from("work/imap/INBOX"),
            kind: String::from("add"),
            seq: Some(5),
        });
        output.parked.push(ParkedAction {
            id: 3,
            collection: String::from("work/imap/INBOX"),
            kind: String::from("set-flags"),
            producer: String::from("himalaya"),
            queued_at: String::from("2026-11-15T08:00:00.000Z"),
            attempts: 1,
            code: None,
            error: String::from("unknown seq 9"),
            sources: Vec::new(),
        });
        let json = serde_json::to_value(&output).unwrap();

        assert_eq!(json["applied"][0]["row"], 2, "{json}");
        assert_eq!(
            json["parked"][0],
            json!({
                "row": 3,
                "collection": "work/imap/INBOX",
                "kind": "set-flags",
                "producer": "himalaya",
                "queuedAt": "2026-11-15T08:00:00.000Z",
                "attempts": 1,
                "code": null,
                "error": "unknown seq 9",
                "sources": [],
            })
        );
    }

    /// An account that could not be drained names its failure as an item
    /// error does, the causes apart from the outermost message.
    #[test]
    fn an_undrained_account_carries_its_causes_apart() {
        let err = anyhow!("no such table").context("Open the store");
        let output = QueueOutput::failed(String::from("work"), err);
        let json = serde_json::to_value(&output).unwrap();

        assert_eq!(json["error"], "Open the store", "{json}");
        assert_eq!(json["sources"], json!(["no such table"]), "{json}");
        let text = output.to_string();
        assert!(
            text.contains("drained: Open the store: no such table"),
            "{text}"
        );
    }
}
