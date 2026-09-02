//! # Neverest
//!
//! A CLI synchronizing PIM collections between backends, and the crate
//! architecture (header-001): the behavioural contract lives in cairn/spec/,
//! the history in cairn/log/, the shared conventions in the [Pimalaya
//! ARCHITECTURE][pimalaya]. Where header and code disagree, the code wins.
//!
//! [pimalaya]: https://github.com/pimalaya/.github/blob/master/ARCHITECTURE.md
//!
//! ## Place in the stack
//!
//! An application, the top of the stack: it writes no protocol and no
//! storage logic of its own, orchestrating the layers below and rendering
//! what they return.
//!
//! The reconcile is [io-replica]'s, which owns the three-way merge, the
//! checkpoints, the push outcomes, the object dedup and the multi-source
//! hub. The local replica is [io-pimdir], a SQLite index beside a
//! content-addressed blob directory implementing io-replica's storage seam.
//!
//! The protocols are io-imap, io-webdav and io-msgraph, behind the lean
//! adapters in [`imap`], [`dav`] and [`msgraph`]. Around them, pimalaya-cli,
//! pimalaya-config and pimalaya-stream supply the CLI, the TOML loading and
//! the blocking runtime; the wizard discovers through [io-pim-discovery].
//!
//! ## The topology mismatch
//!
//! The load-bearing design point. io-replica is local-replica-centric: one
//! local replica against one remote, merged three ways against a
//! per-placement base, its sync verb tying one collection to one remote's
//! enumerate. neverest is peer-to-peer, several sources reaching each other.
//!
//! So calling sync twice cannot replicate both ways. The resolution is the
//! multi-source hub: an account's sources are the sources of one shared
//! collection in one pimdir store, and a load projects the shared item
//! against that source's own base.
//!
//! A change one source folds in therefore reads as locally dirty for the
//! others, whose ordinary reconcile pushes it. Cross-source propagation of
//! items, flags and deletions falls out of the per-source merge, leaving
//! neverest to sequence the coroutines until quiescent.
//!
//! ## Namespaces
//!
//! An account is one hub but not one collection space: a hub collection id
//! is `<namespace>/<name>`, and the namespace is the source's own name, so
//! mail and contacts under one account, or two providers cached side by
//! side, never meet. It is derived, never configured.
//!
//! Which endpoints meet is the account's arity: a pairing binds both into
//! the source's namespace, and an item there with no binding for the other
//! is pushed to it. What the store keeps is `retain`, and a crossing between
//! two streamable endpoints is streamed rather than staged.
//!
//! ## The kind seam
//!
//! Everything above [`client`] is kind-neutral, speaking collections and
//! items rather than mailboxes and messages, so a contacts or calendar
//! backend implements the same surface while each adapter keeps its own
//! nouns behind it: an IMAP mailbox stays a mailbox inside [`imap`].
//!
//! What varies per media type lives in [`kind`]: an item's link id, its
//! versioned summary and sort key, and the three-way merge a content
//! conflict is resolved by, which is here rather than in io-replica so the
//! engine keeps knowing nothing about formats.
//!
//! The kind a source syncs comes from its backend's media type and is
//! recorded on the pimdir collection, so one store may hold several.
//!
//! ## Layout
//!
//! [`cli`] holds the clap parser, one module per subcommand, and the outcome
//! a command exits with: a run that reconciled everything and still left a
//! parked conflict or a refused write is neither a success nor a failure, so
//! it carries a code of its own back to `main`.
//!
//! [`config`] is the TOML schema and [`account`] its runtime counterpart,
//! the endpoints with every secret already resolved, which [`client`], the
//! kind-neutral backend seam, opens a connection from. [`item`] is the
//! vocabulary above that seam, [`kind`] the per-media-type derivations.
//!
//! [`conflict`] is the other end of that merge: the divergences it could not
//! settle, the decision a person makes about one, and the guard refusing a
//! decision the store moved out from under. Deciding is a command and never
//! a run, so nothing there is reachable from a sync.
//!
//! [`offline`] is the sync engine: `mod` maps sources onto pimdir source
//! ids, `state` records what the last run derived, `storage` projects and
//! hydrates one source, `remote` implements io-replica's remote seam,
//! `submit` holds the queued send, and `driver` builds the report.
//!
//! [`sync`] keeps the output types alone, the engine having moved to
//! [`offline`], and [`wizard`] discovers an account from one prompt.
//!
//! What becomes of that account belongs to `cli::configure`, which generates
//! and never edits: it appends the rendered table rather than re-serializing
//! the document, so a hand-written configuration keeps its comments.
//!
//! [`json_schema`] is the registry behind `neverest json-schema`: one entry
//! per data command, mapping its invocation path to the schema of what it
//! prints under `--json`.
//!
//! [io-replica]: https://github.com/pimalaya/io-replica
//! [io-pimdir]: https://github.com/pimalaya/io-pimdir
//! [io-pim-discovery]: https://github.com/pimalaya/io-pim-discovery

mod account;
mod cli;
mod client;
mod config;
mod conflict;
#[cfg(feature = "dav")]
mod dav;
#[cfg(feature = "imap")]
mod imap;
mod item;
mod json_schema;
mod kind;
#[cfg(feature = "msgraph")]
mod msgraph;
mod offline;
mod sync;
mod wizard;

use std::{
    io::{IsTerminal, stdin},
    path::PathBuf,
    process::ExitCode,
};

use anyhow::Result;
use clap::{CommandFactory, Parser};
use pimalaya_cli::{
    error::ErrorReport,
    log::Logger,
    printer::{Printer, StdoutPrinter},
};
use pimalaya_config::toml::TomlConfig;

use crate::{
    cli::{configure::offer_configuration, exit::Exit, main::Cli},
    config::Config,
};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let mut printer = StdoutPrinter::new(&cli.json);
    let result = execute(&mut printer, cli);

    // NOTE: the outcome that is neither success nor failure comes back here
    // rather than exiting inside a subcommand, so one place decides it.
    ErrorReport::eval(&mut printer, result).into()
}

fn execute(printer: &mut StdoutPrinter, cli: Cli) -> Result<Exit> {
    Logger::try_init(&cli.log)?;
    let config_paths = cli.config_paths.as_ref();

    match cli.command {
        Some(command) => command.execute(printer, config_paths, cli.account.name.as_deref()),
        None => meet_bare_invocation(printer, config_paths, cli.account.name.is_some())
            .map(|()| Exit::Success),
    }
}

/// Meets a bare `neverest`, which is where a newcomer lands.
///
/// A missing configuration raises the offer; anything else gets the help.
/// A broken file counts as a configuration, so the offer never writes over
/// one, and `--account` alone is a half-typed command, not a first run.
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

        if offer_configuration(printer, config_paths, &target)? {
            return Ok(());
        }
    }

    Cli::command().print_help()?;

    Ok(())
}
