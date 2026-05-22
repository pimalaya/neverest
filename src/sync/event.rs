//! Progress events emitted by [`crate::sync::builder::SyncBuilder::run`].
//!
//! The variants mirror the v1 surface so the indicatif handler that
//! drove the v1 sync (`MultiProgress` + per-mailbox `ProgressBar`)
//! ports almost verbatim — only the field rename `folder → mailbox`
//! changes.

use std::collections::HashMap;

use crate::sync::hunk::EmailHunk;

/// Streamed status of a running sync. The handler is called
/// synchronously between work steps, so it must be cheap (indicatif
/// updates only).
#[derive(Clone, Debug)]
pub enum SyncEvent {
    /// Both sides have returned their mailbox list. The engine is
    /// about to compute the mailbox patch.
    ListedAllMailboxes,

    /// Mailbox patch applied. The engine is about to start listing
    /// envelopes per mailbox.
    ProcessedAllMailboxHunks,

    /// Per-mailbox envelope patch generated. Carries the full count
    /// table so the progress UI can spawn per-mailbox bars up front.
    GeneratedEmailPatch(HashMap<String, Vec<EmailHunk>>),

    /// One envelope hunk applied. Carries the hunk for display.
    ProcessedEmailHunk(EmailHunk),

    /// Every envelope hunk applied. The engine is about to expunge
    /// trashed messages on each side that supports it.
    ProcessedAllEmailHunks,

    /// Final tick. The engine has finished and is about to return the
    /// [`crate::sync::report::SyncReport`].
    Done,
}

/// Sync event sink. `()` is provided when callers don't care about
/// progress (e.g. `--dry-run` users that consume the
/// [`crate::sync::report::SyncReport`] directly).
pub trait Handler: Send {
    fn handle(&mut self, event: SyncEvent) -> anyhow::Result<()>;
}

impl Handler for () {
    fn handle(&mut self, _event: SyncEvent) -> anyhow::Result<()> {
        Ok(())
    }
}

impl<F: FnMut(SyncEvent) -> anyhow::Result<()> + Send> Handler for F {
    fn handle(&mut self, event: SyncEvent) -> anyhow::Result<()> {
        (self)(event)
    }
}
