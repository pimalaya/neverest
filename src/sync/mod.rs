//! Report DTOs shared with the printer.
//!
//! The reconcile engine moved to [`crate::offline`] (io-replica). What remains
//! here is the user-facing report shape: [`hunk`] describes one applied change
//! and [`report`] aggregates them into the printed `SyncReport`. The driver
//! translates what io-replica did back into these DTOs so the CLI output stays
//! stable.

pub mod hunk;
pub mod report;
