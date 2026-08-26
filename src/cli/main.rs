//! Top-level CLI parser and subcommand dispatcher.

use std::path::PathBuf;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use pimalaya_cli::{
    clap::{
        args::{AccountFlag, JsonFlag, LogFlags},
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
    /// The command to run; a bare `neverest` (no subcommand) runs the
    /// configuration wizard instead.
    #[command(subcommand)]
    pub command: Option<Command>,

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
    pub account: AccountFlag,
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
    #[command(alias = "manuals")]
    Manual(ManualCommand),
    #[command(arg_required_else_help = true)]
    #[command(alias = "completions")]
    Completion(CompletionCommand),
}

impl Command {
    /// Runs the subcommand against the account `-a` names, or the default one
    /// when it names none.
    ///
    /// The flag is global and declared once, on [`Cli`], as it is in every
    /// other pimalaya CLI: which account a command runs against is a property
    /// of the invocation, not of the subcommand, and repeating it per
    /// subcommand is how the same option ends up documented four ways.
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        account: Option<&str>,
    ) -> Result<()> {
        match self {
            Self::Check(cmd) => cmd.execute(printer, config_paths, account),
            Self::Init(cmd) => cmd.execute(printer, config_paths, account),
            Self::Sync(cmd) => cmd.execute(printer, config_paths, account),
            Self::Configure(cmd) => cmd.execute(printer, config_paths, account),
            Self::Manual(cmd) => cmd.execute(printer, Cli::command()),
            Self::Completion(cmd) => cmd.execute(printer, Cli::command()),
        }
    }
}
