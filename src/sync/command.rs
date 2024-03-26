//! # Synchronize backends command
//!
//! This module contains the [`clap`] command for synchronizing two
//! backends.

use anyhow::Result;
use clap::Parser;
use log::info;

use crate::{config::TomlConfig, preset::arg::name::OptionalPresetNameArg, printer::Printer};

/// Synchronize folders and emails of two different backend sources.
#[derive(Debug, Parser)]
pub struct SynchronizeBackendsCommand {
    #[command(flatten)]
    pub preset: OptionalPresetNameArg,
}

impl SynchronizeBackendsCommand {
    pub async fn execute(self, printer: &mut impl Printer, config: &TomlConfig) -> Result<()> {
        info!("executing synchronize emails command");

        printer.print(format!("{config:#?}"))?;

        Ok(())
    }
}
