//! # Neverest CLI

pub mod account;
pub mod backend;
pub mod cli;
pub mod completion;
pub mod config;
#[cfg(feature = "imap")]
pub mod imap;
#[cfg(feature = "maildir")]
pub mod maildir;
pub mod manual;
#[cfg(feature = "notmuch")]
pub mod notmuch;
pub mod output;
pub mod printer;
pub mod ui;
