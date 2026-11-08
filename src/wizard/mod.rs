//! # Wizard
//!
//! The interactive configuration: one prompt discovering an account, a
//! review of what each domain picked, the sign-ins completing it, the
//! collections each source syncs, and the account it comes out as.

pub mod discover;
#[cfg(feature = "imap")]
pub mod imap;
pub mod plan;
pub mod search;
pub mod secret;
pub mod server;
pub mod settings;
pub mod signin;
#[cfg(feature = "smtp")]
pub mod smtp;
