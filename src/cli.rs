use clap::{Parser, Subcommand};
use color_eyre::{eyre::Result, Section};
use std::{env, path::PathBuf};
use tracing_subscriber::filter::LevelFilter;

use crate::{
    account::command::{
        configure::ConfigureAccountCommand, doctor::DoctorAccountCommand,
        sync::SynchronizeAccountCommand,
    },
    completion::command::GenerateCompletionCommand,
    config::{self, Config},
    manual::command::GenerateManualCommand,
    output::{ColorFmt, OutputFmt},
    printer::{Printer, StdoutPrinter},
};

#[derive(Parser, Debug)]
#[command(name = "neverest", author, version, about)]
#[command(propagate_version = true, infer_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: NeverestCommand,

    /// Override the default configuration file path.
    ///
    /// The given paths are shell-expanded then canonicalized (if
    /// applicable). If the first path does not point to a valid file,
    /// the wizard will propose to assist you in the creation of the
    /// configuration file. Other paths are merged with the first one,
    /// which allows you to separate your public config from your
    /// private(s) one(s).
    #[arg(short, long = "config", global = true)]
    #[arg(value_name = "PATH", value_parser = config::path_parser)]
    pub config_paths: Vec<PathBuf>,

    /// Customize the output format.
    ///
    /// The output format determine how to display commands output to
    /// the terminal.
    ///
    /// The possible values are:
    ///
    ///  - json: output will be in a form of a JSON-compatible object
    ///
    ///  - plain: output will be in a form of either a plain text or
    ///    table, depending on the command
    #[arg(long, short, global = true)]
    #[arg(value_name = "FORMAT", value_enum, default_value_t = Default::default())]
    pub output: OutputFmt,

    /// Control when to use colors
    ///
    /// The default setting is 'auto', which means neverest will try
    /// to guess when to use colors. For example, if neverest is
    /// printing to a terminal, then it will use colors, but if it is
    /// redirected to a file or a pipe, then it will suppress color
    /// output. neverest will suppress color output in some other
    /// circumstances as well. For example, if the TERM environment
    /// variable is not set or set to 'dumb', then neverest will not
    /// use colors.
    ///
    /// The possible values are:
    ///
    ///  - never: colors will never be used
    ///
    ///  - always: colors will always be used regardless of where output is sent
    ///
    ///  - ansi: like 'always', but emits ANSI escapes (even in a Windows console)
    ///
    ///  - auto: neverest tries to be smart
    #[arg(long, short = 'C', global = true)]
    #[arg(value_name = "MODE", value_enum, default_value_t = Default::default())]
    pub color: ColorFmt,

    /// Enable logs with spantrace.
    ///
    /// This is the same as running the command with `RUST_LOG=debug`
    /// environment variable.
    #[arg(long, global = true, conflicts_with = "trace")]
    pub debug: bool,

    /// Enable verbose logs with backtrace.
    ///
    /// This is the same as running the command with `RUST_LOG=trace`
    /// and `RUST_BACKTRACE=1` environment variables.
    #[arg(long, global = true, conflicts_with = "debug")]
    pub trace: bool,
}

impl Cli {
    pub async fn execute(self) -> Result<()> {
        if env::var("RUST_LOG").is_err() {
            if self.debug {
                env::set_var("RUST_LOG", "debug");
            } else if self.trace {
                env::set_var("RUST_LOG", "trace");
            }
        }

        let filter = crate::tracing::install()?;

        let mut printer = StdoutPrinter::new(self.output, self.color);

        let mut res = self
            .command
            .execute(&mut printer, self.config_paths.as_ref())
            .await;

        if filter < LevelFilter::DEBUG {
            res = res.note("Run with --debug to enable logs with spantrace.");
        }

        if filter < LevelFilter::TRACE {
            res = res.note("Run with --trace to enable verbose logs with backtrace.")
        }

        res
    }
}

#[derive(Subcommand, Debug)]
pub enum NeverestCommand {
    #[command(alias = "check-up", alias = "checkup", visible_alias = "check")]
    Doctor(DoctorAccountCommand),

    #[command(alias = "cfg")]
    Configure(ConfigureAccountCommand),

    #[command(alias = "synchronise")]
    Synchronize(SynchronizeAccountCommand),

    #[command(arg_required_else_help = true)]
    #[command(alias = "manuals", alias = "mans")]
    Manual(GenerateManualCommand),

    #[command(arg_required_else_help = true)]
    #[command(alias = "completions")]
    Completion(GenerateCompletionCommand),
}

impl NeverestCommand {
    pub async fn execute(self, printer: &mut impl Printer, config_paths: &[PathBuf]) -> Result<()> {
        match self {
            Self::Doctor(cmd) => {
                let config = Config::from_paths_or_default(config_paths).await?;
                cmd.execute(printer, &config).await
            }
            Self::Configure(cmd) => {
                let config = Config::from_paths_or_default(config_paths).await?;
                cmd.execute(printer, &config).await
            }
            Self::Synchronize(cmd) => {
                let config = Config::from_paths_or_default(config_paths).await?;
                cmd.execute(printer, &config).await
            }
            Self::Manual(cmd) => cmd.execute(printer).await,
            Self::Completion(cmd) => cmd.execute().await,
        }
    }
}
