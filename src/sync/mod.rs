//! # Sync report DTOs
//!
//! The user-facing report shape: [`hunk`] describes one applied change and
//! [`report`] aggregates them into the printed `SyncOutput`.
//!
//! The reconcile itself is [`crate::offline`] (io-pimdir); the driver
//! translates what the engine did back into these DTOs, so the CLI output
//! stays stable.

pub mod hunk;
pub mod report;
