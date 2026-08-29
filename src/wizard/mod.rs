//! # Wizard
//!
//! Interactive configuration: email-driven service discovery, per-backend
//! credential prompts, and the converters from answers to [`crate::config`].

#[cfg(feature = "dav")]
pub mod dav;
pub mod discover;
#[cfg(feature = "imap")]
pub mod imap_smtp;
#[cfg(feature = "msgraph")]
pub mod msgraph;
pub mod search;
pub mod secret;
