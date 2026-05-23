//! `neverest convert <format>` command tree.
//!
//! Per-format converters live in sibling modules (`convert::maildir`,
//! later `convert::mbox` and friends). This file holds the clap
//! parsing surface that dispatches between them so adding a new
//! source format is one new module + one new variant on
//! [`ConvertSubcommand`].

use std::path::PathBuf;

use anyhow::Result;
#[cfg(not(feature = "m2dir"))]
use anyhow::bail;
use clap::{Parser, Subcommand};
use pimalaya_cli::printer::Printer;

/// Convert mail from a foreign on-disk format into an m2store.
/// One-shot migrators, idempotent on re-run.
#[derive(Subcommand, Debug)]
#[command(arg_required_else_help = true)]
pub enum ConvertCommand {
    /// Convert a Maildir(++) tree into an m2store.
    Maildir(MaildirCommand),
}

impl ConvertCommand {
    pub fn execute(self, printer: &mut impl Printer) -> Result<()> {
        match self {
            ConvertCommand::Maildir(cmd) => cmd.execute(printer),
        }
    }
}

/// One-shot Maildir(++) -> m2store converter. Walks a Maildir(++)
/// root, translates folder names (`.Work.Foo` -> `Work/Foo`; root
/// cur/new/tmp -> `INBOX`), and copies every message into the
/// destination m2store with its flag sidecar reconstructed from the
/// info-section letters, an optional per-folder `dovecot-keywords`
/// table, and an optional named header.
#[derive(Parser, Debug)]
pub struct MaildirCommand {
    /// Path to the source Maildir(++) root.
    #[arg(value_name = "SOURCE")]
    pub source: PathBuf,

    /// Path to the destination m2store root.
    #[arg(value_name = "DEST")]
    pub destination: PathBuf,

    /// Also read keywords from a per-message header. When set, the named header
    /// is stripped from the bytes written to the destination m2dir; its values
    /// feed the flag sidecar.
    #[arg(long, value_enum, value_name = "HEADER")]
    pub read_headers: Option<HeaderSource>,
}

/// Per-message header source for keyword recovery.
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum HeaderSource {
    /// OfflineIMAP-style `X-Keywords: foo, bar` (comma-separated).
    XKeywords,
    /// Mutt / notmuch-style `X-Label: foo bar` (space-separated).
    XLabel,
}

impl MaildirCommand {
    #[cfg(feature = "m2dir")]
    pub fn execute(self, printer: &mut impl Printer) -> Result<()> {
        super::maildir::run(printer, self.source, self.destination, self.read_headers)
    }

    #[cfg(not(feature = "m2dir"))]
    pub fn execute(self, _printer: &mut impl Printer) -> Result<()> {
        bail!("`convert maildir` requires the `m2dir` feature; rebuild with `--features m2dir`");
    }
}
