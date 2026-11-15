//! # Sync report DTOs
//!
//! The user-facing report shape: [`hunk`] describes one applied change and
//! [`report`] aggregates them into the printed `SyncOutput`.
//!
//! The reconcile itself is [`crate::offline`] (io-pimdir); the driver
//! translates what the engine did back into these DTOs, so the CLI output
//! stays stable.

pub mod event;
pub mod hunk;
pub mod report;

use std::sync::OnceLock;

use anyhow::Result;
use pimalaya_cli::signal::Shutdown;

/// The shutdown a sync polls, once `sync` installed it.
static SHUTDOWN: OnceLock<Shutdown> = OnceLock::new();

/// Makes the first SIGINT or SIGTERM ask the run to stop rather than end
/// the process, so it prints what it did (pimalaya-cli
/// `signal::Shutdown`).
pub fn install_shutdown() -> Result<()> {
    if SHUTDOWN.get().is_none() {
        let _ = SHUTDOWN.set(Shutdown::install()?);
    }

    Ok(())
}

/// Whether a termination signal asked the run to stop: the loops start no
/// new collection, batch or account, and the account's report reads as
/// interrupted.
pub fn stopping() -> bool {
    SHUTDOWN.get().is_some_and(Shutdown::is_requested)
}
