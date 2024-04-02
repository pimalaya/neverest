//! # Account commands
//!
//! This module gathers CLI commands dedicated to accounts:
//! [`check_up`] to check up the validity of a given account,
//! [`configure`] to configure secrets of a given account, and
//! [`sync`] to synchronize two backends of a given account.

pub mod check_up;
pub mod configure;
pub mod sync;
