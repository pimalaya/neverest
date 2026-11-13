//! # CLI
//!
//! Clap-driven: top-level parser, one module per subcommand, and the outcome
//! a run exits with.

pub mod check;
pub mod conflict;
pub mod drain;
pub mod exit;
#[cfg(feature = "search")]
pub mod index;
pub mod init;
pub mod main;
pub mod sync;
