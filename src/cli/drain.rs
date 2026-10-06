//! # Drain command
//!
//! Applies what frontends queued in the store, opening no endpoint, so
//! they read their own writes back without waiting for a sync.

use std::{fmt, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use io_pimdir::client::producer::PimdirActionStatus;
use log::info;
use pimalaya_cli::printer::Printer;
use pimalaya_config::toml::TomlConfig;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    cli::{exit::Exit, sync::try_store_lock},
    config::Config,
    offline::driver,
    sync::report::SyncOutput,
};

/// Applies the store changes frontends queued, without syncing.
///
/// A frontend (himalaya, calendula, cardamum) queues its writes, and
/// only the store's owner applies them: until then a created item has
/// no id and no reader lists it. This applies them now, the way a sync
/// does first, reading no credential and opening no endpoint, so it
/// works offline. The next sync pushes them.
///
/// Intents, a send or an invitation reply among them, need their server
/// and stay queued for the sync. A sync holding the store answers
/// `busy` with exit code 3: that run, or the next, applies them.
#[derive(Debug, Parser)]
pub struct DrainCommand {}

impl DrainCommand {
    /// Drains the account's store, or reports it busy.
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

        // NOTE: never waits: a sync holding the store drains it itself, at
        // its start or at the next run.
        let Some(_lock) = try_store_lock(&dir)
            .with_context(|| format!("Acquire the store lock of account {name}"))?
        else {
            printer.out(DrainOutput::busy(name))?;
            return Ok(Exit::Incomplete);
        };

        let mode = account_config.mode()?;
        let first = mode
            .sources
            .first()
            .expect("a validated account has at least one source");

        // NOTE: as a sync does before its drain, from the configuration
        // alone (pimdir STORAGE §15.6).
        driver::declare(&dir, &name, &account_config, &mode)?;

        let mut store = driver::open_store(&dir, first, &name)?;
        let queued = store
            .list_pending_actions()
            .context("List the queued actions")?;

        let mut report = SyncOutput::default();
        driver::drain_queues(&mut store, &mut report);

        let mut output = DrainOutput::new(name);
        for row in queued {
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

        info!(
            "drained {} queued action(s) of {} ({} parked, {} waiting)",
            output.applied.len(),
            output.account,
            output.parked.len(),
            output.waiting,
        );

        printer.out(output)?;
        Ok(Exit::Success)
    }
}

/// What `neverest drain` did with the account's queue.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DrainOutput {
    /// The account whose store was drained.
    pub account: String,
    /// A sync held the store, so nothing was drained.
    pub busy: bool,
    /// The actions applied to the store, in queue order.
    pub applied: Vec<DrainedAction>,
    /// The actions refused for good this time, kept for an operator.
    pub parked: Vec<ParkedAction>,
    /// The rows left queued for a sync: intents, a send or a reply, and
    /// actions a transient failure deferred.
    pub waiting: usize,
}

impl DrainOutput {
    fn new(account: String) -> Self {
        Self {
            account,
            busy: false,
            applied: Vec::new(),
            parked: Vec::new(),
            waiting: 0,
        }
    }

    fn busy(account: String) -> Self {
        Self {
            busy: true,
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
        if self.waiting > 0 {
            writeln!(f, "{} left queued for the next sync", self.waiting)?;
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
    #[serde(skip_serializing_if = "Option::is_none")]
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
