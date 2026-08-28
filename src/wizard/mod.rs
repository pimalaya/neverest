//! Interactive configuration wizard: email-driven service discovery,
//! per-backend credential prompts, and the converters from wizard
//! answers to on-disk [`crate::config`] types.

#[cfg(feature = "dav")]
pub mod dav;
pub mod discover;
pub mod edit;
#[cfg(feature = "imap")]
pub mod imap_smtp;
#[cfg(feature = "msgraph")]
pub mod msgraph;
pub mod search;
pub mod secret;
