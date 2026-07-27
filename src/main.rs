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
//! peer-to-peer: two remote sides, reconciled so a change on either one
//! reaches the other.
//!
//! So calling sync twice cannot produce bidirectional replication. The
//! resolution is io-replica's multi-source hub. The two sides are two sources
//! of one shared collection in one pimdir store, and a load projects the
//! shared item against that source's own base. A change one side folds into
//! the hub therefore reads as locally dirty for the other, whose ordinary
//! reconcile pushes it. Cross-side propagation of items, flags and deletions
//! falls out of the per-side merge, with no hand-rolled cross-merge, leaving
//! neverest to sequence the coroutines until quiescent.
//!
//! ## The kind seam
//!
//! Everything above [`client`] is kind-neutral. It speaks collections and
//! items rather than mailboxes and messages, so a contacts or calendar backend
//! implements the same surface, while each adapter keeps its own protocol
//! nouns behind it: an IMAP mailbox stays a mailbox inside [`imap`]. Exactly
//! two things vary per media type, and both live in [`kind`], an item's link
//! id and its versioned summary. The kind a side syncs comes from the
//! backend's media type and is recorded on the pimdir collection, so one store
//! may hold several.
//!
//! ## Layout
//!
//! The [`cli`] module holds the clap parser and one module per subcommand.
//! [`config`] is the TOML schema, [`client`] the kind-neutral backend seam
//! opening one side, [`item`] the vocabulary above that seam, and [`kind`] the
//! per-media-type derivations. [`side`] tags a side left or right.
//!
//! [`offline`] is the sync engine: `mod` maps sides onto pimdir source ids and
//! drives the coroutines, `hash` content-addresses bodies, `storage` projects
//! and hydrates one side over a pimdir store, `remote` implements io-replica's
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
mod side;
mod sync;
mod wizard;

use anyhow::Result;
use clap::Parser;
use pimalaya_cli::{error::ErrorReport, log::Logger, printer::StdoutPrinter};
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
        Some(command) => command.execute(printer, config_paths),
        None => {
            discover::run(printer, &Config::target_path(config_paths)?)?;
            Ok(())
        }
    }
}
