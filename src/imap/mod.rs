//! # IMAP backend
//!
//! A thin wrapper around io-imap's high-level session, plus the adapter that
//! maps its wire types onto the shared [`crate::item`] domain types.

pub mod backend;
pub mod client;
