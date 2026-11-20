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
//! The reconcile and the local replica are both [io-pimdir]'s: the engine
//! owns the three-way merge, the checkpoints, the push outcomes, the object
//! dedup and the multi-source hub, and the store is a SQLite index beside a
//! content-addressed blob directory servicing the engine's storage seam.
//!
//! The protocols are io-imap, io-webdav and io-msgraph, behind the lean
//! adapters in [`imap`], [`dav`] and [`msgraph`]. Around them, pimalaya-cli,
//! pimalaya-config and pimalaya-stream supply the CLI, the TOML loading and
//! the blocking runtime.
//!
//! ## The topology mismatch
//!
//! The load-bearing design point. The engine is local-replica-centric: one
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
//! Every account shares one store (`<state base>/neverest/`, or the
//! `store.root` accounts name), each one hub within it, its collections
//! grouped under its name.
//!
//! Nor is an account one collection space: a source id is
//! `<account>/<source>` and a hub collection id `<account>/<source>/<name>`,
//! so same-named sources of two accounts, kinds or providers never meet.
//! Derived, never configured, it is stripped before any wire call or report.
//!
//! One run covers every account, or those `-a` names, one after the other
//! under the store's one lock (pimdir allows one owner process a store).
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
//! typed summary and sort key, which io-pimdir derives, and the three-way
//! merge a content conflict is resolved by, which is here so the engine
//! keeps knowing nothing about merging formats.
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
//! secrets resolved, which [`client`], the kind-neutral backend seam, opens
//! a connection from. [`collection`] and [`flag`] are the vocabulary above
//! that seam, [`kind`] the per-media-type derivations.
//!
//! [`conflict`] is the other end of that merge: the divergences it could not
//! settle, the decision a person makes about one, and the guard refusing a
//! decision the store moved out from under. Deciding is a command and never
//! a run, so nothing there is reachable from a sync.
//!
//! [`sync`] is the sync engine: `mod` maps sources onto pimdir source ids,
//! `state` records, per account, what the last run derived, `storage`
//! projects and hydrates one source, `remote` implements io-pimdir's remote
//! seam, `submit` holds the queued send, and `driver` builds the report.
//!
//! [`json_schema`] is the registry behind `neverest json-schema`: one entry
//! per data command, mapping its invocation path to the schema of what it
//! prints under `--json`. [`error`] names the failures a caller acts on, a
//! store failure by the code io-pimdir gives it.
//!
//! [io-pimdir]: https://github.com/pimalaya/io-pimdir

mod account;
#[cfg(any(feature = "gcal", feature = "msgraph"))]
mod calendar;
mod cli;
mod client;
mod collection;
mod config;
mod conflict;
#[cfg(any(feature = "carddav", feature = "caldav"))]
mod dav;
mod error;
mod flag;
#[cfg(feature = "gcal")]
mod gcal;
#[cfg(feature = "gmail")]
mod gmail;
#[cfg(feature = "gpeople")]
mod gpeople;
#[cfg(any(
    feature = "gcal",
    feature = "gpeople",
    feature = "gmail",
    feature = "msgraph"
))]
mod http;
#[cfg(feature = "imap")]
mod imap;
mod json_schema;
mod kind;
#[cfg(feature = "msgraph")]
mod msgraph;
mod notice;
#[cfg(any(
    feature = "imap",
    feature = "smtp",
    feature = "carddav",
    feature = "caldav",
    feature = "msgraph",
    feature = "gpeople",
    feature = "gcal",
    feature = "gmail"
))]
mod proxy;
#[cfg(any(feature = "imap", feature = "smtp"))]
mod sasl;
#[cfg(all(
    test,
    any(
        feature = "carddav",
        feature = "caldav",
        feature = "msgraph",
        feature = "gcal",
        feature = "gpeople",
        feature = "gmail"
    )
))]
mod scripted;
mod server;
mod sync;
mod terminal;
mod throttle;
#[cfg(any(
    feature = "imap",
    feature = "smtp",
    feature = "carddav",
    feature = "caldav",
    feature = "msgraph",
    feature = "gpeople",
    feature = "gcal",
    feature = "gmail"
))]
mod tls;
#[cfg(any(feature = "search", feature = "gcal", feature = "msgraph"))]
mod zone;

use std::{
    io::{self, IsTerminal},
    process::{self, ExitCode},
};

use anyhow::Result;
use clap::CommandFactory;
use pimalaya_cli::{
    clap::{meet_bare_invocation, parse},
    error::{EXIT_FAILURE, ErrorReport},
    log::Logger,
    printer::{Printer, StdoutPrinter},
    welcome::{is_wanted, shipped_sample},
};
use pimalaya_config::toml::ConfigMissingError;
use simd_cpu::SimdCpu;

use crate::{
    cli::{
        exit::Exit,
        main::{self as cli_main, Cli, Command},
    },
    config::Config,
    error::ErrorCode,
    terminal::TerminalPrinter,
};

/// Parses and runs the command, then decides its outcome in this one place.
///
/// The SIMD CPU token goes first. A full sync streams its events and
/// progress lines, piped too, every other command on a terminal alone. A
/// person missing a configuration meets the welcome, a script the error.
fn main() -> ExitCode {
    let _ = SimdCpu::install_detected();

    let cli = parse::<Cli>();
    let mut printer = StdoutPrinter::new(&cli.json);
    let streams = cli.command.as_ref().is_some_and(Command::streams);
    if cli.json.enabled || !(streams || io::stdout().is_terminal()) {
        notice::silence();
    }

    if streams {
        printer = printer.streaming();
    }
    let mut printer = TerminalPrinter(printer);

    let config_paths = cli.config.paths.clone();
    let result = execute(&mut printer, cli);
    let result = result.map_err(error::coded);

    if let Err(err) = &result
        && let Some(missing) = err.downcast_ref::<ConfigMissingError>()
        && is_wanted(&printer)
    {
        let sample = shipped_sample("neverest");
        let welcome = cli_main::welcome(&missing.path, &config_paths, sample.as_deref());
        eprint!("{welcome}");
        process::exit(EXIT_FAILURE);
    }

    ErrorReport::<ErrorCode>::eval_coded(&mut printer, result).into()
}

/// Runs the subcommand, or meets a bare `neverest`.
fn execute(printer: &mut (impl Printer + Send), cli: Cli) -> Result<Exit> {
    Logger::try_init(&cli.log)?;
    let config_paths = &cli.config.paths;

    match cli.command {
        Some(command) => command.execute(printer, config_paths, &cli.accounts),
        None => {
            meet_bare_invocation(printer, Cli::command(), || {
                let missing = Config::load(config_paths).err();
                missing.filter(|err| err.is::<ConfigMissingError>())
            })?;
            Ok(Exit::Success)
        }
    }
}
