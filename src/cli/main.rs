//! # Parser
//!
//! Top-level CLI parser and subcommand dispatcher.

use std::path::PathBuf;

use anyhow::Result;
use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use pimalaya_cli::{
    clap::{
        args::{JsonFlag, LogFlags},
        commands::{
            completion::CompletionCommand, json_schema::JsonSchemaCommand, manual::ManualCommand,
        },
        parsers::path_parser,
    },
    long_version,
    printer::Printer,
};

#[cfg(feature = "search")]
use crate::cli::index::IndexCommand;
use crate::{
    cli::{
        check::CheckCommand, conflict::ConflictCommand, drain::DrainCommand, exit::Exit,
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
    /// The command to run; a bare `neverest` prints the help.
    #[command(subcommand)]
    pub command: Option<Command>,
    /// Override the default configuration file path.
    ///
    /// Paths are shell-expanded then canonicalized; multiple paths may be
    /// delimited by `:` and are merged left-to-right.
    #[arg(short, long = "config", global = true, env = "NEVEREST_CONFIG")]
    #[arg(value_name = "PATH", value_parser = path_parser, value_delimiter = ':')]
    pub config_paths: Vec<PathBuf>,
    /// The accounts to run over, every configured one when none is named.
    ///
    /// Repeatable. An account is named by the key of its `[accounts.<name>]`
    /// table. Every account shares one store unless it names its own
    /// `store.root`.
    #[arg(long = "account", short = 'a', global = true)]
    #[arg(value_name = "NAME", action = ArgAction::Append)]
    pub accounts: Vec<String>,
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
    Drain(DrainCommand),
    #[cfg(feature = "search")]
    Index(IndexCommand),
    #[command(arg_required_else_help = true)]
    #[command(alias = "conflicts")]
    Conflict(ConflictCommand),
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
    /// Runs the subcommand over the accounts `-a` names, or every one.
    ///
    /// The flag is global and declared once, on [`Cli`], as in every other
    /// pimalaya CLI: the accounts are a property of the invocation. Sync,
    /// drain, init and index have an outcome beyond succeeding or failing, so
    /// they return their own [`Exit`]; every other works or errors.
    pub fn execute(
        self,
        printer: &mut (impl Printer + Send),
        config_paths: &[PathBuf],
        accounts: &[String],
    ) -> Result<Exit> {
        let done = match self {
            Self::Sync(cmd) => return cmd.execute(printer, config_paths, accounts),
            Self::Drain(cmd) => return cmd.execute(printer, config_paths, accounts),
            #[cfg(feature = "search")]
            Self::Index(cmd) => return cmd.execute(printer, config_paths, accounts),
            Self::Init(cmd) => return cmd.execute(printer, config_paths, accounts),
            Self::Check(cmd) => cmd.execute(printer, config_paths, accounts),
            Self::Conflict(cmd) => cmd.execute(printer, config_paths, accounts),
            Self::Manual(cmd) => cmd.execute(printer, Cli::command()),
            Self::Completion(cmd) => cmd.execute(printer, Cli::command()),
            Self::JsonSchema(cmd) => cmd.execute(printer, json_schema::registry()),
        };

        done.map(|()| Exit::Success)
    }
}
