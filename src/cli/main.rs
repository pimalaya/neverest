//! # Parser
//!
//! Top-level CLI parser and subcommand dispatcher.

use std::path::PathBuf;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use pimalaya_cli::{
    clap::{
        args::{AccountFlag, JsonFlag, LogFlags},
        commands::{CompletionCommand, JsonSchemaCommand, ManualCommand},
        parsers::path_parser,
    },
    long_version,
    printer::Printer,
};

use crate::{
    cli::{
        check::CheckCommand, configure::ConfigureCommand, conflict::ConflictCommand, exit::Exit,
        init::InitCommand, sync::SyncCommand,
    },
    json_schema,
};

/// The command line neverest parses, and the flags every subcommand shares.
#[derive(Parser, Debug)]
#[command(name = env!("CARGO_PKG_NAME"))]
#[command(author, version, about)]
#[command(long_version = long_version!())]
#[command(propagate_version = true, infer_subcommands = true)]
pub struct Cli {
    /// The command to run; a bare `neverest` prints the help, or offers the
    /// configuration wizard when no configuration file is found.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Override the default configuration file path.
    ///
    /// Paths are shell-expanded then canonicalized; multiple paths may be
    /// delimited by `:` and are merged left-to-right. When none resolves to
    /// an existing file, the wizard runs against the first.
    #[arg(short, long = "config", global = true, env = "NEVEREST_CONFIG")]
    #[arg(value_name = "PATH", value_parser = path_parser, value_delimiter = ':')]
    pub config_paths: Vec<PathBuf>,
    /// The account to run against, defaulting to the one claiming `default`.
    #[command(flatten)]
    pub account: AccountFlag,
    /// Whether to print the command payload as JSON rather than as text.
    #[command(flatten)]
    pub json: JsonFlag,
    /// How verbose the logs on stderr are.
    #[command(flatten)]
    pub log: LogFlags,
}

/// Every verb the binary offers, each one its own command type.
#[derive(Debug, Subcommand)]
pub enum Command {
    Check(CheckCommand),
    Init(InitCommand),
    Sync(SyncCommand),
    #[command(arg_required_else_help = true)]
    #[command(alias = "conflicts")]
    Conflict(ConflictCommand),
    #[command(alias = "cfg")]
    Configure(ConfigureCommand),
    #[command(arg_required_else_help = true)]
    #[command(alias = "manuals")]
    Manual(ManualCommand),
    #[command(arg_required_else_help = true)]
    #[command(alias = "completions")]
    Completion(CompletionCommand),
    #[command(alias = "json-schemas")]
    JsonSchema(JsonSchemaCommand),
}

impl Command {
    /// Runs the subcommand against the account `-a` names, or the default.
    ///
    /// The flag is global and declared once, on [`Cli`], as in every other
    /// pimalaya CLI: the account is a property of the invocation. Sync is the
    /// one command with an outcome beyond succeeding or failing, so it
    /// returns its own [`Exit`]; every other works or errors.
    pub fn execute(
        self,
        printer: &mut impl Printer,
        config_paths: &[PathBuf],
        account: Option<&str>,
    ) -> Result<Exit> {
        let done = match self {
            Self::Sync(cmd) => return cmd.execute(printer, config_paths, account),
            Self::Check(cmd) => cmd.execute(printer, config_paths, account),
            Self::Conflict(cmd) => cmd.execute(printer, config_paths, account),
            Self::Init(cmd) => cmd.execute(printer, config_paths, account),
            Self::Configure(cmd) => cmd.execute(printer, config_paths),
            Self::Manual(cmd) => cmd.execute(printer, Cli::command()),
            Self::Completion(cmd) => cmd.execute(printer, Cli::command()),
            Self::JsonSchema(cmd) => cmd.execute(printer, json_schema::schemas()),
        };

        done.map(|()| Exit::Success)
    }
}
