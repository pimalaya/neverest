//! # Sync report DTOs
//!
//! The user-facing report shape: [`hunk`] describes one applied change and
//! [`report`] aggregates them into the printed `SyncReport`.
//!
//! The reconcile itself is [`crate::offline`] (io-replica); the driver
//! translates what the engine did back into these DTOs, so the CLI output
//! stays stable.

pub mod hunk;
pub mod report;
