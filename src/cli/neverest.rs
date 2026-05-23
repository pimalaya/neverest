//! CLI entry point.
//!
//! Mirrors [`himalaya::cli::HimalayaCli`] in shape: global args
//! (`--config`, `--account`, `--json`, `--log-*`) flatten into the
//! `clap::Parser`; a [`NeverestCommand`] subcommand carries the
//! per-subcommand state and is dispatched by [`NeverestCommand::execute`].

use std::path::PathBuf;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use pimalaya_cli::{
    clap::{
        args::{JsonFlag, LogFlags},
        commands::{CompletionCommand, ManualCommand},
        parsers::path_parser,
    },
    long_version,
    printer::Printer,
};

use crate::{
    cli::{check::CheckCommand, configure::ConfigureCommand, init::InitCommand},
    convert::cli::ConvertCommand,
    sync::cli::SyncCommand,
};

#[derive(Parser, Debug)]
#[command(name = env!("CARGO_PKG_NAME"))]
#[command(author, version, about)]
#[command(long_version = long_version!())]
#[command(propagate_version = true, infer_subcommands = true)]
pub struct NeverestCli {
    #[command(subcommand)]
    pub command: NeverestCommand,

    /// Override the default configuration file path.
    ///
    /// The given paths are shell-expanded then canonicalized (if
    /// applicable). Multiple paths can be supplied delimited by `:`
    /// and are merged left-to-right (later paths override earlier
    /// ones). When no path resolves to an existing file, the
    /// configuration wizard runs against the first one.
    #[arg(short, long = "config", global = true, env = "NEVEREST_CONFIG")]
    #[arg(value_name = "PATH", value_parser = path_parser, value_delimiter = ':')]
    pub config_paths: Vec<PathBuf>,
    #[command(flatten)]
    pub json: JsonFlag,
    #[command(flatten)]
    pub log: LogFlags,
}

#[derive(Debug, Subcommand)]
pub enum NeverestCommand {
    Check(CheckCommand),
    Init(InitCommand),
    Sync(SyncCommand),
    #[command(alias = "cfg")]
    Configure(ConfigureCommand),

    #[command(subcommand)]
    Convert(ConvertCommand),

    #[command(arg_required_else_help = true)]
    Manuals(ManualCommand),
    #[command(arg_required_else_help = true)]
    Completions(CompletionCommand),
}

impl NeverestCommand {
    pub fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        match self {
            Self::Init(cmd) => cmd.execute(printer, config_paths),
            Self::Sync(cmd) => cmd.execute(printer, config_paths),
            Self::Check(cmd) => cmd.execute(printer, config_paths),
            Self::Configure(cmd) => cmd.execute(printer, config_paths),
            Self::Convert(cmd) => cmd.execute(printer),
            Self::Manuals(cmd) => cmd.execute(printer, NeverestCli::command()),
            Self::Completions(cmd) => cmd.execute(printer, NeverestCli::command()),
        }
    }
}
