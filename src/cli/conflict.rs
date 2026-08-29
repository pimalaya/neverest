//! `neverest conflict` command: lists the divergences runs parked, shows the
//! bodies one is between, and settles it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{ArgGroup, Parser, Subcommand};
use io_pimdir::PimdirReader;
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
        let store = read(&name, &store_dir(&name, &account_config)?)?;

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
        let store = read(&name, &store_dir(&name, &account_config)?)?;

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
        let dir = store_dir(&name, &account_config)?;

        self.resolve(printer, &name, &account_config, &dir)
    }

    /// The decision loop, from the bodies a merger is handed to the edit that
    /// settles the divergence.
    ///
    /// Nothing here holds the store open across the decision. io-pimdir's
    /// owner lock lives on the handle, so a handle kept for the command's
    /// whole life would refuse every sync of that store for as long as a
    /// person sits in an editor, which is exactly the window the staleness
    /// guard was written for: the bodies are read through a lock-free reader
    /// that is dropped before the merger runs, and the store is opened again,
    /// under neverest's own `sync.lock`, only to apply what came back.
    fn resolve(
        &self,
        printer: &mut impl Printer,
        name: &str,
        account_config: &AccountConfig,
        dir: &Path,
    ) -> Result<()> {
        for attempt in 1..=MAX_ATTEMPTS {
            let (conflict, sides) = {
                let store = read(name, dir)?;
                let conflicts = conflict::list(&store, name)?;
                let conflict = conflict::find(conflicts, self.id, self.source.as_deref())?;

                if !conflict.resolvable() {
                    bail!(
                        "Conflict {} is waiting for its diverging body, which the next sync fetches",
                        conflict.id
                    );
                }

                let sides = conflict.sides(&store.blobs())?;

                (conflict, sides)
            };

            let Some(body) = self.decide(account_config, &conflict, sides)? else {
                return printer.out(ConflictResolveOutput::Aborted { id: conflict.id });
            };

            let _lock = acquire_store_lock(dir, LOCK_TIMEOUT)?;

            match conflict.apply(dir, name, &body)? {
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

/// The account's store directory, refusing one no `init` has created.
fn store_dir(name: &str, account_config: &AccountConfig) -> Result<PathBuf> {
    let dir = driver::store_dir(name, account_config)?;

    if !dir.join("pimdir.db").exists() {
        bail!("Account {name} not initialized, run `init -a {name}` first");
    }

    Ok(dir)
}

/// Opens the account's store for reading only.
///
/// A reader owns nothing and takes no lock (pimdir SPEC §8), so any number of
/// them run against a store a sync is holding. Every conflict command reads
/// through one: listing what is parked, showing the three bodies and handing
/// them to a merger are all reads, and none of them is a reason to keep a
/// sync out.
fn read(name: &str, dir: &Path) -> Result<PimdirReader> {
    PimdirReader::open(dir).with_context(|| format!("Read the store of account {name}"))
}

#[cfg(test)]
mod tests {
    use std::{
        fmt, fs, thread,
        time::{Duration, Instant},
    };

    use anyhow::Result;
    use io_pimdir::{PimdirSourceStore, PimdirStore};
    use io_replica::{
        change::ReplicaWriteOp,
        client::ReplicaStorage,
        collection::ReplicaCollectionId,
        object::ReplicaObject,
        placement::{
            ReplicaBase, ReplicaFlags, ReplicaHandle, ReplicaLevel, ReplicaLinkId, ReplicaMeta,
            ReplicaPlacement, ReplicaSortKey, ReplicaStatus,
        },
    };
    use serde::Serialize;

    use super::*;
    use crate::offline::storage::load_side;

    /// The account the seeded store is grouped under.
    const ACCOUNT: &str = "cards";

    /// The identity the seeded card states and the placement is linked by.
    const UID: &str = "uid:a";

    /// The revision the divergence was recorded at, which is the one the
    /// concurrent sync moves the store past.
    const REVISION: &str = "etag-2";

    /// How long a step of the handshake waits before failing the test rather
    /// than hanging the suite.
    const PATIENCE: Duration = Duration::from_secs(10);

    /// A [`Printer`] keeping what it was handed, so the test reads the
    /// command's own output rather than the store alone.
    #[derive(Default)]
    struct TestPrinter(String);

    impl Printer for TestPrinter {
        fn out<T: fmt::Display + Serialize>(&mut self, data: T) -> Result<()> {
            self.0 = data.to_string();
            Ok(())
        }
    }

    /// A card carrying one phone number, stating the identity it is linked by.
    fn card(tel: &str) -> String {
        format!(
            "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:{UID}\r\nFN:Jane Doe\r\nTEL:{tel}\r\nEND:VCARD\r\n"
        )
    }

    /// Seeds a store holding one card the engine marked conflicted, with all
    /// three bodies present, which is the state a decision is made from.
    fn store_with_conflict(dir: &Path) -> PimdirSourceStore {
        let mut store = PimdirStore::open(dir)
            .unwrap()
            .for_account(ACCOUNT)
            .for_source("dav");
        store.ensure_collection("contacts", "text/vcard").unwrap();

        let blobs = store.blobs();
        let stored = |body: String| ReplicaWriteOp::StoreObject {
            object: ReplicaObject {
                hash: blobs.hash(body.as_bytes()),
                size: body.len(),
            },
            body: Some(body.into_bytes()),
        };

        store
            .write(vec![
                stored(card("+1")),
                stored(card("+2")),
                stored(card("+3")),
                ReplicaWriteOp::UpsertPlacement(ReplicaPlacement {
                    collection: ReplicaCollectionId("contacts".into()),
                    handle: ReplicaHandle("card1".into()),
                    link_id: Some(ReplicaLinkId(UID.into())),
                    object: Some(blobs.hash(card("+2").as_bytes())),
                    level: ReplicaLevel::Full,
                    meta: Some(ReplicaMeta(r#"{"v":1}"#.into())),
                    sort_key: ReplicaSortKey::default(),
                    flags: ReplicaFlags::default(),
                    status: ReplicaStatus::Conflict,
                    conflict_revision: Some(String::from(REVISION)),
                    conflict_object: Some(blobs.hash(card("+3").as_bytes())),
                    base: Some(ReplicaBase {
                        flags: ReplicaFlags::default(),
                        revision: Some(String::from("etag-1")),
                        object: Some(blobs.hash(card("+1").as_bytes())),
                    }),
                    origin: None,
                }),
            ])
            .unwrap();

        store
    }

    /// A sync runs while a person is in the merger, and the decision they
    /// come back with is recomputed against what arrived.
    ///
    /// This is the whole point of the staleness guard, and it was
    /// unreachable: the command held io-pimdir's owner lock for its entire
    /// life, so the only thing that can move a placement's conflict revision,
    /// a sync of that store, was refused outright while the merger was up.
    /// `Applied::Moved`, the retry loop and the warning that goes with it
    /// were all dead in ordinary use.
    ///
    /// The merger here is that sync: it signals the test, waits for the
    /// store to be written from another thread as a sync would write it, and
    /// only then answers. The write is what fails before the fix.
    #[cfg(unix)]
    #[test]
    fn a_store_written_under_the_merger_sends_the_decision_back_for_another_look() {
        let dir = tempfile::tempdir().unwrap();
        let scripts = tempfile::tempdir().unwrap();
        drop(store_with_conflict(dir.path()));

        let entered = scripts.path().join("entered");
        let go = scripts.path().join("go");
        let attempts = scripts.path().join("attempts");
        let merger = scripts.path().join("merger.sh");
        fs::write(
            &merger,
            format!(
                "#!/bin/sh\n\
                 echo . >> {attempts}\n\
                 touch {entered}\n\
                 waited=0\n\
                 while [ ! -e {go} ]; do\n\
                   waited=$((waited + 1))\n\
                   [ \"$waited\" -gt 1000 ] && exit 3\n\
                   sleep 0.01\n\
                 done\n\
                 cp \"$2\" \"$4\"\n",
                attempts = attempts.display(),
                entered = entered.display(),
                go = go.display(),
            ),
        )
        .unwrap();

        let config: AccountConfig =
            toml::from_str(&format!("conflict.merger = \"sh {}\"", merger.display())).unwrap();

        let watcher = {
            let entered = entered.clone();
            let go = go.clone();
            let dir = dir.path().to_path_buf();
            thread::spawn(move || {
                await_file(&entered);

                // The lock itself, and not only the handle: io-pimdir counts
                // owning handles per process, so a second `open` here would
                // succeed off that count whether or not the file lock is
                // free. This is the lock another process contends for.
                let owner = fs::File::options()
                    .read(true)
                    .write(true)
                    .open(dir.join("owner.lock"))
                    .unwrap();
                owner
                    .try_lock()
                    .expect("the store is unowned while the merger runs");
                drop(owner);

                // What no sync could do while the merger was up: take the
                // store and record the revision the remote moved to.
                let mut store = PimdirStore::open(&dir)
                    .expect("a store the merger does not own")
                    .for_account(ACCOUNT)
                    .for_source("dav");
                let mut placement = load_side(&store, "contacts").unwrap().remove(0);
                placement.conflict_revision = Some(String::from("etag-3"));
                store
                    .write(vec![ReplicaWriteOp::UpsertPlacement(placement)])
                    .unwrap();
                drop(store);

                fs::write(&go, b"").unwrap();
            })
        };

        let command = ConflictResolveCommand {
            id: 1,
            source: None,
            prefer_local: false,
            prefer_remote: false,
            interactive: true,
        };
        let mut printer = TestPrinter::default();
        command
            .resolve(&mut printer, ACCOUNT, &config, dir.path())
            .unwrap();
        watcher
            .join()
            .expect("the store is written under the merger");

        assert_eq!(
            fs::read_to_string(&attempts).unwrap().lines().count(),
            2,
            "the decision is exported again once the store moves under it",
        );
        assert!(printer.0.contains("Settled conflict 1"), "{}", printer.0);

        let store = PimdirStore::open(dir.path())
            .unwrap()
            .for_account(ACCOUNT)
            .for_source("dav");
        let placement = load_side(&store, "contacts").unwrap().remove(0);
        assert_ne!(placement.status, ReplicaStatus::Conflict);
        assert_eq!(
            placement.object,
            Some(store.blobs().hash(card("+2").as_bytes())),
            "settled with the body the merger wrote, which is the local side",
        );
    }

    /// Polls for a file the merger writes, bounded so a failure is a failed
    /// test rather than a hung suite.
    fn await_file(path: &Path) {
        let deadline = Instant::now() + PATIENCE;
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "{} never appeared",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
