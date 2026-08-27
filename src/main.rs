//! neverest, a CLI synchronizing PIM collections between two backends.
//!
//! Parses the CLI, configures logging, dispatches the requested subcommand.
//! This header is the crate architecture (header-001), the behavioural
//! contract lives in cairn/spec/, and the reviewable history in cairn/changes/
//! and cairn/log/. Read the [Pimalaya ARCHITECTURE][pimalaya] first for the
//! conventions every repository shares. Where this header and the code
//! disagree, the code wins; please flag it.
//!
//! [pimalaya]: https://github.com/pimalaya/.github/blob/master/ARCHITECTURE.md
//!
//! ## Place in the stack
//!
//! neverest is an application, the top of the Pimalaya stack. It writes no
//! protocol and no storage logic of its own: it orchestrates the layers below
//! and renders their results.
//!
//! The reconcile itself belongs to [io-replica], the I/O-free offline-first
//! replica engine owning the three-way merge, the checkpoints, the
//! push-outcome discipline, the object dedup and the multi-source hub. The
//! local replica belongs to [io-pimdir], a SQLite index beside a
//! content-addressed blob directory implementing io-replica's storage seam.
//! The protocols belong to io-imap and io-msgraph, driven by the lean adapters
//! in [`imap`] and [`msgraph`]. The wizard discovers accounts through
//! [io-pim-discovery]. Around all of it, pimalaya-cli, pimalaya-config and
//! pimalaya-stream supply the clap arguments, printer, logger and prompts, the
//! TOML loading, and the blocking runtime carrying TLS and SASL.
//!
//! [io-replica]: https://github.com/pimalaya/io-replica
//! [io-pimdir]: https://github.com/pimalaya/io-pimdir
//! [io-pim-discovery]: https://github.com/pimalaya/io-pim-discovery
//!
//! ## The topology mismatch
//!
//! The load-bearing design point, because the two shapes do not obviously fit.
//! io-replica is local-replica-centric: one local replica reconciled against
//! one remote, through a three-way merge against a per-placement base. Its
//! sync verb ties one collection to one remote's enumerate, and never
//! propagates an item from one collection to another. neverest is
//! peer-to-peer: several remote sources, reconciled so a change on any one
//! reaches the others.
//!
//! So calling sync twice cannot produce bidirectional replication. The
//! resolution is io-replica's multi-source hub. An account's sources are the
//! sources of one shared collection in one pimdir store, and a load projects
//! the shared item against that source's own base. A change one source folds
//! into the hub therefore reads as locally dirty for the others, whose ordinary
//! reconcile pushes it. Cross-source propagation of items, flags and deletions
//! falls out of the per-source merge, with no hand-rolled cross-merge, leaving
//! neverest to sequence the coroutines until quiescent.
//!
//! ## Namespaces
//!
//! An account is one hub, but not one collection space. Sources of one kind
//! sharing a `collection.namespace` bind the same hub collections, and that
//! sharing is what propagation is: an item sitting in a collection a source
//! participates in, with no binding for that source, is pushed to it. A
//! namespace defaults to the source's own name, so sources are isolated until
//! someone points two at the same one. That is the whole difference between a
//! mirror and two providers cached side by side, and it is why mail, contacts
//! and calendar live under one account without meeting.
//!
//! What the store keeps follows from the same fact and is never configured: a
//! source alone in its namespace keeps every body, a streamable pair keeps
//! none and streams each crossing, anything else keeps what crossed. Every run
//! and `neverest check` report it.
//!
//! ## The kind seam
//!
//! Everything above [`client`] is kind-neutral. It speaks collections and
//! items rather than mailboxes and messages, so a contacts or calendar backend
//! implements the same surface, while each adapter keeps its own protocol
//! nouns behind it: an IMAP mailbox stays a mailbox inside [`imap`]. Exactly
//! two things vary per media type, and both live in [`kind`], an item's link
//! id and its versioned summary. The kind a source syncs comes from the
//! backend's media type and is recorded on the pimdir collection, so one store
//! may hold several.
//!
//! ## Layout
//!
//! The [`cli`] module holds the clap parser and one module per subcommand.
//! [`config`] is the TOML schema, [`client`] the kind-neutral backend seam
//! opening one source, [`item`] the vocabulary above that seam, and [`kind`]
//! the per-media-type derivations.
//!
//! [`offline`] is the sync engine: `mod` maps sources onto pimdir source ids
//! and drives the coroutines, `state` records what the last run derived,
//! `storage` projects and hydrates one source over a pimdir store, `remote`
//! implements io-replica's
//! remote seam over one client, `submit` holds the queued submit intent and
//! its send channel (mail alone), and `driver` orchestrates an account and builds
//! the report. [`sync`] keeps only the report types, the engine having moved
//! to [`offline`]. [`wizard`] bootstraps a first configuration.

#[cfg(feature = "carddav")]
mod carddav;
mod cli;
mod client;
mod config;
#[cfg(feature = "imap")]
mod imap;
mod item;
mod kind;
#[cfg(feature = "msgraph")]
mod msgraph;
mod offline;
mod sync;
mod wizard;

use std::{
    io::{IsTerminal, stdin},
    path::PathBuf,
};

use anyhow::Result;
use clap::{CommandFactory, Parser};
use pimalaya_cli::{
    error::ErrorReport,
    log::Logger,
    printer::{Printer, StdoutPrinter},
};
use pimalaya_config::toml::TomlConfig;

use crate::{cli::main::Cli, config::Config, wizard::discover};

fn main() {
    let cli = Cli::parse();
    let mut printer = StdoutPrinter::new(&cli.json);
    let result = execute(&mut printer, cli);
    ErrorReport::eval(&mut printer, result);
}

fn execute(printer: &mut StdoutPrinter, cli: Cli) -> Result<()> {
    Logger::try_init(&cli.log)?;
    let config_paths = cli.config_paths.as_ref();

    match cli.command {
        Some(command) => command.execute(printer, config_paths, cli.account.name.as_deref()),
        None => meet_bare_invocation(printer, config_paths, cli.account.name.is_some()),
    }
}

/// Meets a bare `neverest`, which is where a newcomer lands.
///
/// With no command there is nothing to run: a missing configuration
/// raises the offer, and an existing one gets the help, which is also
/// what a script or a JSON caller gets since neither can answer a prompt.
/// A file that exists but fails to parse counts as a configuration, so
/// the offer never proposes to write over a broken one: the parse error
/// surfaces when a real command reads it.
///
/// `--account` names an account to act on, so with no subcommand it is a
/// half-typed command rather than a first run: it gets the help, which
/// points at the commands, instead of an offer to create an account.
fn meet_bare_invocation(
    printer: &mut StdoutPrinter,
    config_paths: &[PathBuf],
    named_account: bool,
) -> Result<()> {
    let configured = Config::from_paths_or_default(config_paths)
        .ok()
        .flatten()
        .is_some();

    if !configured && !named_account && !printer.is_json() && stdin().is_terminal() {
        let target = Config::target_path(config_paths)?;

        if discover::offer_configuration(printer, &target)? {
            return Ok(());
        }
    }

    Cli::command().print_help()?;

    Ok(())
}
