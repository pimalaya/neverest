//! # CLI
//!
//! Clap-driven: top-level parser, one module per subcommand, and the outcome
//! a run exits with.

pub mod check;
#[cfg(feature = "wizard")]
pub mod configure;
pub mod conflict;
pub mod exit;
pub mod init;
pub mod main;
pub mod sync;

#[cfg(not(feature = "wizard"))]
use std::path::{Path, PathBuf};

#[cfg(not(feature = "wizard"))]
use anyhow::Result;
#[cfg(not(feature = "wizard"))]
use pimalaya_cli::printer::Printer;

/// Offers nothing in a build without the wizard, so the caller falls back
/// to what it does when the offer is declined.
#[cfg(not(feature = "wizard"))]
pub fn offer_configuration(
    _printer: &mut impl Printer,
    _config_paths: &[PathBuf],
    _path: &Path,
) -> Result<bool> {
    Ok(false)
}
