//! # Account
//!
//! An account is a backend tuple (left and right) identified by a
//! name. The [`arg`] and [`command`] modules contain CLI
//! account-related arguments and commands. The [`config`] module
//! contains its associated user configuration. Finally, the
//! [`wizard`] module contains code to generate an account
//! configuration.

pub mod arg;
pub mod command;
pub mod config;
#[cfg(feature = "wizard")]
pub mod wizard;
