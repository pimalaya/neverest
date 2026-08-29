//! `neverest conflict` command: lists the divergences runs parked, shows the
//! bodies one is between, and settles it.

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{ArgGroup, Parser, Subcommand};
use io_pimdir::PimdirStore;
use log::warn;
use pimalaya_cli::printer::Printer;
use pimalaya_config::toml::TomlConfig;

use crate::{
    cli::sync::{LOCK_TIMEOUT, acquire_store_lock},
    config::{AccountConfig, Config},
    conflict::{
        self, Applied, Conflict, Sides,
        merger::Merger,
        report::{ConflictListOutput, ConflictResolveOutput, ConflictShowOutput, ConflictSummary},
    },
    offline::driver,
};

/// How many times a decision is recomputed against a remote that moved under
/// it before the command gives up.
///
/// The retry is what makes the guard usable rather than a wall: a person who
/// spent a minute in a merger is handed the bodies that arrived meanwhile and
/// asked again. The cap is what keeps a store somebody else is syncing hard
/// from turning that into a loop.
const MAX_ATTEMPTS: usize = 3;

/// Lists, inspects and settles the divergences a run could not merge away.
///
/// Deciding is a command and never a run: nothing here is reached from a
/// sync, whatever is attached to its terminal.
#[derive(Debug, Parser)]
pub struct ConflictCommand {
    #[command(subcommand)]
    pub command: ConflictSubcommand,
}

/// The three things a person does about a divergence.
#[derive(Debug, Subcommand)]
pub enum ConflictSubcommand {
    List(ConflictListCommand),
    Show(ConflictShowCommand),
    Resolve(ConflictResolveCommand),
}

impl ConflictCommand {
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        account_name: Option<&str>,
    ) -> Result<()> {
        match self.command {
            ConflictSubcommand::List(cmd) => cmd.execute(printer, config_paths, account_name),
            ConflictSubcommand::Show(cmd) => cmd.execute(printer, config_paths, account_name),
            ConflictSubcommand::Resolve(cmd) => cmd.execute(printer, config_paths, account_name),
        }
    }
}

/// Lists the items waiting for a decision, whichever run parked them.
///
/// An item whose diverging remote body no run has fetched yet is listed and
/// is not resolvable until one has.
#[derive(Debug, Parser)]
pub struct ConflictListCommand {}

impl ConflictListCommand {
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        account_name: Option<&str>,
    ) -> Result<()> {
        let (name, account_config) = account(printer, config_paths, account_name)?;
        let (store, _) = open(&name, &account_config)?;

        let conflicts = conflict::list(&store, &name)?
            .iter()
            .map(ConflictSummary::from)
            .collect();

        printer.out(ConflictListOutput { conflicts })
    }
}

/// Shows one divergence and the three bodies it is between: the base the last
/// sync agreed on, and what each side made of it.
#[derive(Debug, Parser)]
pub struct ConflictShowCommand {
    /// The item's public id, as `conflict list` shows it.
    #[arg(value_name = "ID")]
    pub id: i64,

    /// The source the divergence is on, for an item that diverged on more
    /// than one.
    #[arg(long, short = 's', value_name = "SOURCE")]
    pub source: Option<String>,
}

impl ConflictShowCommand {
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        account_name: Option<&str>,
    ) -> Result<()> {
        let (name, account_config) = account(printer, config_paths, account_name)?;
        let (store, _) = open(&name, &account_config)?;

        let conflicts = conflict::list(&store, &name)?;
        let conflict = conflict::find(conflicts, self.id, self.source.as_deref())?;
        let sides = conflict.sides(&store.blobs())?;

        printer.out(ConflictShowOutput::new(&conflict, sides))
    }
}

/// Settles one divergence, by taking a side or by handing the bodies to the
/// configured merger.
///
/// `--prefer-local` and `--prefer-remote` discard the other side, which is
/// acceptable because a person asked for it by name and is exactly what a
/// background run must never do on its own. The decision is refused when the
/// store has observed a newer remote revision since it was computed.
#[derive(Debug, Parser)]
#[command(group = ArgGroup::new("side").required(true))]
pub struct ConflictResolveCommand {
    /// The item's public id, as `conflict list` shows it.
    #[arg(value_name = "ID")]
    pub id: i64,

    /// The source the divergence is on, for an item that diverged on more
    /// than one.
    #[arg(long, short = 's', value_name = "SOURCE")]
    pub source: Option<String>,

    /// Keep the store's body and discard the remote's.
    #[arg(long, group = "side")]
    pub prefer_local: bool,

    /// Keep the remote's body and discard the store's.
    #[arg(long, group = "side")]
    pub prefer_remote: bool,

    /// Hand the three bodies to the `conflict.merger` command and take back
    /// the one it writes.
    #[arg(long, short = 'i', group = "side")]
    pub interactive: bool,
}

impl ConflictResolveCommand {
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        account_name: Option<&str>,
    ) -> Result<()> {
        let (name, account_config) = account(printer, config_paths, account_name)?;
        let (store, dir) = open(&name, &account_config)?;
        let blobs = store.blobs();

        for attempt in 1..=MAX_ATTEMPTS {
            let conflicts = conflict::list(&store, &name)?;
            let conflict = conflict::find(conflicts, self.id, self.source.as_deref())?;

            if !conflict.resolvable() {
                bail!(
                    "Conflict {} is waiting for its diverging body, which the next sync fetches",
                    conflict.id
                );
            }

            let sides = conflict.sides(&blobs)?;

            let Some(body) = self.decide(&account_config, &conflict, sides)? else {
                return printer.out(ConflictResolveOutput::Aborted { id: conflict.id });
            };

            // The lock is taken here rather than around the whole command, so
            // a sync is free to run while a person is in a merger. What that
            // costs is exactly what the staleness guard answers.
            let _lock = acquire_store_lock(&dir, LOCK_TIMEOUT)?;

            match conflict.apply(&dir, &name, &body)? {
                Applied::Resolved => {
                    return printer.out(ConflictResolveOutput::Resolved {
                        id: conflict.id,
                        collection: conflict.collection,
                        side: String::from(self.side()),
                    });
                }
                Applied::Settled => bail!(
                    "Conflict {} was settled while the decision was being made, so nothing was pushed",
                    conflict.id
                ),
                Applied::Moved(revision) => {
                    let revision = revision.unwrap_or_else(|| String::from("an unnamed one"));

                    if !self.interactive || attempt == MAX_ATTEMPTS {
                        bail!(
                            "The remote of conflict {} moved to revision {revision} while the decision was being made, so nothing was pushed",
                            conflict.id
                        );
                    }

                    warn!(
                        "the remote of conflict {} moved to revision {revision}, exporting it again",
                        conflict.id
                    );
                }
            }
        }

        bail!(
            "The remote of conflict {} keeps moving under the decision, so nothing was pushed",
            self.id
        )
    }

    /// The body this decision settles on, or `None` when the merger aborted.
    fn decide(
        &self,
        account_config: &AccountConfig,
        conflict: &Conflict,
        sides: Sides,
    ) -> Result<Option<Vec<u8>>> {
        if self.prefer_local {
            let Some(body) = sides.local else {
                bail!(
                    "The local side of conflict {} is not in the store",
                    conflict.id
                );
            };

            return Ok(Some(body));
        }

        if self.prefer_remote {
            let Some(body) = sides.remote else {
                bail!(
                    "The remote side of conflict {} is not in the store",
                    conflict.id
                );
            };

            return Ok(Some(body));
        }

        let Some(command) = &account_config.conflict.merger else {
            bail!("No interactive merger is configured, name one with `conflict.merger`");
        };

        let kind = conflict.kind()?;
        let dir = tempfile::Builder::new()
            .prefix("neverest-conflict-")
            .tempdir()?;

        Merger::export(command, dir.path(), kind.extension(), &sides)?.run()
    }

    /// The side the decision took, for the report.
    fn side(&self) -> &'static str {
        if self.prefer_local {
            "local"
        } else if self.prefer_remote {
            "remote"
        } else {
            "merged"
        }
    }
}

/// Loads the configuration and takes the account the invocation names.
fn account(
    printer: &mut impl Printer,
    config_paths: &[PathBuf],
    account_name: Option<&str>,
) -> Result<(String, AccountConfig)> {
    let mut config = Config::load_or_wizard(printer, config_paths)?;

    let Some((name, account_config)) = config.take_account(account_name)? else {
        bail!("Cannot find account");
    };

    account_config.validate()?;

    Ok((name, account_config))
}

/// Opens the account's store, refusing one no `init` has created, and returns
/// it beside the directory a resolution stages its edit through.
fn open(name: &str, account_config: &AccountConfig) -> Result<(PimdirStore, PathBuf)> {
    let dir = driver::store_dir(name, account_config)?;

    if !dir.join("pimdir.db").exists() {
        bail!("Account {name} not initialized, run `init -a {name}` first");
    }

    let store = PimdirStore::open(&dir)?.for_account(name);

    Ok((store, dir))
}
