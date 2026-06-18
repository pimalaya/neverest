//! Top-level CLI parser and subcommand dispatcher.

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

use crate::cli::{
    check::CheckCommand, configure::ConfigureCommand, init::InitCommand, sync::SyncCommand,
};

#[derive(Parser, Debug)]
#[command(name = env!("CARGO_PKG_NAME"))]
#[command(author, version, about)]
#[command(long_version = long_version!())]
#[command(propagate_version = true, infer_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Override the default configuration file path.
    ///
    /// Paths are shell-expanded then canonicalized; multiple paths may
    /// be delimited by `:` and are merged left-to-right. When no path
    /// resolves to an existing file, the wizard runs against the first
    /// one.
    #[arg(short, long = "config", global = true, env = "NEVEREST_CONFIG")]
    #[arg(value_name = "PATH", value_parser = path_parser, value_delimiter = ':')]
    pub config_paths: Vec<PathBuf>,
    #[command(flatten)]
    pub json: JsonFlag,
    #[command(flatten)]
    pub log: LogFlags,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Check(CheckCommand),
    Init(InitCommand),
    Sync(SyncCommand),
    #[command(alias = "cfg")]
    Configure(ConfigureCommand),
    #[command(arg_required_else_help = true)]
    Manuals(ManualCommand),
    #[command(arg_required_else_help = true)]
    Completions(CompletionCommand),
}

impl Command {
    pub fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        match self {
            Self::Check(cmd) => cmd.execute(printer, config_paths),
            Self::Init(cmd) => cmd.execute(printer, config_paths),
            Self::Sync(cmd) => cmd.execute(printer, config_paths),
            Self::Configure(cmd) => cmd.execute(printer, config_paths),
            Self::Manuals(cmd) => cmd.execute(printer, Cli::command()),
            Self::Completions(cmd) => cmd.execute(printer, Cli::command()),
        }
    }
}
