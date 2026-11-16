//! # Parser
//!
//! Top-level CLI parser and subcommand dispatcher, and the first-run
//! welcome a bare invocation or a command meets without a configuration.

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use pimalaya_cli::{
    clap::{
        args::{ConfigFlags, JsonFlag, LogFlags},
        commands::{
            completion::CompletionCommand, json_schema::JsonSchemaCommand, manual::ManualCommand,
        },
    },
    footer, long_version,
    printer::Printer,
    welcome::Welcome,
};
use pimalaya_config::toml::ConfigMissingError;

use crate::{
    cli::{
        account::AccountCommand, check::CheckCommand, conflict::ConflictCommand, exit::Exit,
        init::InitCommand, sync::SyncCommand,
    },
    config::Config,
    json_schema,
};

/// The sample configuration as a raw file, for `curl` to fetch.
const SAMPLE_RAW_URL: &str =
    "https://raw.githubusercontent.com/pimalaya/neverest/master/config.sample.toml";

/// The command line neverest parses, and the flags every subcommand shares.
#[derive(Parser, Debug)]
#[command(name = env!("CARGO_PKG_NAME"))]
#[command(author, version, about)]
#[command(long_version = long_version!())]
#[command(after_help = footer!())]
#[command(propagate_version = true, infer_subcommands = true)]
pub struct Cli {
    /// The command to run, a bare `neverest` showing this help.
    #[command(subcommand)]
    pub command: Option<Command>,
    #[command(flatten)]
    pub config: ConfigFlags,
    /// The accounts to run over, every configured one when none is named.
    ///
    /// Repeatable, unlike the other Pimalaya CLIs: a run covers several
    /// accounts. An account is named by the key of its `[accounts.<name>]`
    /// table. Every account shares one store unless it names its own
    /// `store.root`.
    #[arg(long = "account", short = 'a', global = true)]
    #[arg(value_name = "NAME", action = ArgAction::Append)]
    pub accounts: Vec<String>,
    #[command(flatten)]
    pub json: JsonFlag,
    #[command(flatten)]
    pub log: LogFlags,
}

/// Every verb the binary offers, each one its own command type.
#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(subcommand)]
    Account(AccountCommand),
    /// Probe every configured source, as `account check` does.
    ///
    /// The name `account check` had before the account group, hidden from
    /// the help so a script calling it runs on.
    #[command(hide = true)]
    Check(CheckCommand),
    Init(InitCommand),
    Sync(SyncCommand),
    #[command(arg_required_else_help = true)]
    #[command(alias = "conflicts")]
    Conflict(ConflictCommand),
    #[command(alias = "completions")]
    Completion(CompletionCommand),
    #[command(alias = "manuals")]
    Manual(ManualCommand),
    #[command(alias = "json-schemas")]
    JsonSchema(JsonSchemaCommand),
}

impl Command {
    /// Runs the subcommand over the accounts `-a` names, or every one.
    ///
    /// The flag is global and declared once, on [`Cli`], as in every other
    /// pimalaya CLI: the accounts are a property of the invocation. Sync
    /// and init have an outcome beyond succeeding or failing, so they return
    /// their own [`Exit`]; every other works or errors.
    pub fn execute(
        self,
        printer: &mut (impl Printer + Send),
        config_paths: &[PathBuf],
        accounts: &[String],
    ) -> Result<Exit> {
        let done = match self {
            Self::Sync(cmd) => return cmd.execute(printer, config_paths, accounts),
            Self::Init(cmd) => return cmd.execute(printer, config_paths, accounts),
            Self::Account(cmd) => cmd.execute(printer, config_paths, accounts),
            Self::Check(cmd) => cmd.execute(printer, config_paths, accounts),
            Self::Conflict(cmd) => cmd.execute(printer, config_paths, accounts),
            Self::Completion(cmd) => cmd.execute(printer, Cli::command()),
            Self::Manual(cmd) => cmd.execute(printer, Cli::command()),
            Self::JsonSchema(cmd) => cmd.execute(printer, json_schema::registry()),
        };

        done.map(|()| Exit::Success)
    }
}

/// Meets a bare `neverest`: the help when a configuration exists, the
/// missing configuration otherwise, which a person meets as the welcome.
pub fn meet_bare_invocation(config_paths: &[PathBuf]) -> Result<()> {
    // NOTE: a broken file counts as a configuration, its error left to
    // the first command reading it.
    if let Err(err) = Config::load(config_paths)
        && err.is::<ConfigMissingError>()
    {
        return Err(err);
    }

    Cli::command().print_help()?;
    Ok(())
}

/// The welcome a person meets when no configuration exists at `path`:
/// the commands creating it from `sample`, the shipped sample
/// configuration, or downloading it without one, then `account check`.
pub fn welcome<'a>(
    path: &'a Path,
    config_paths: &'a [PathBuf],
    sample: Option<&'a Path>,
) -> Welcome<'a> {
    Welcome {
        bin: "neverest",
        intro: "Welcome to Neverest, the CLI to synchronize PIM collections.",
        path,
        config_paths,
        sample,
        sample_url: SAMPLE_RAW_URL,
        next: "Edit it and uncomment the options you need, then check your accounts:",
        command: "account check",
    }
}
