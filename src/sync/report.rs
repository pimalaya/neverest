//! End-of-run summary returned by
//! [`crate::sync::builder::SyncBuilder::run`].
//!
//! Each hunk is paired with its application error (`None` on success)
//! so the caller can render a single "what failed" block at the end
//! instead of polling the handler for errors mid-run.

use anyhow::Error;

use crate::sync::hunk::{EmailHunk, MailboxHunk};

#[derive(Debug, Default)]
pub struct SyncReport {
    pub mailbox: PatchOutcome<MailboxHunk>,
    pub email: PatchOutcome<EmailHunk>,
}

#[derive(Debug)]
pub struct PatchOutcome<H> {
    pub patch: Vec<(H, Option<Error>)>,
}

impl<H> Default for PatchOutcome<H> {
    fn default() -> Self {
        Self { patch: Vec::new() }
    }
}
