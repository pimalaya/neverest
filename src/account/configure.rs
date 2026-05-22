//! `neverest configure` command.
//!
//! Prompts/refreshes the keyring secrets referenced by the left and
//! right sides of the configured account. Secret resolution itself
//! lives inside [`pimalaya_config::secret::Secret`]; this command is
//! a thin wrapper that walks the config tree and prompts when a
//! secret has an empty keyring slot.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use pimalaya_cli::printer::Printer;

#[derive(Debug, Parser)]
pub struct ConfigureAccountCommand {
    #[arg(value_name = "ACCOUNT")]
    pub account: Option<String>,

    /// Forget the stored secret(s) before prompting for new values.
    #[arg(long, short = 'r')]
    pub reset: bool,
}

impl ConfigureAccountCommand {
    pub fn execute(self, printer: &mut impl Printer, _config_paths: &[PathBuf]) -> Result<()> {
        // Concrete prompt/store wiring depends on the keyring API
        // pimalaya-cli ends up exposing for non-interactive secret
        // refresh. Keeping this command structured but inert until
        // that surface lands so users still see the subcommand in
        // --help and the CLI compiles end-to-end.
        let _ = self;
        printer.out(
            "`configure` is not yet implemented against the new pimalaya-config secret backend.",
        )?;
        Ok(())
    }
}
