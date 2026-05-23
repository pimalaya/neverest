//! End-of-run summary returned by [`crate::sync::engine::run`].
//!
//! Each hunk is paired with its application error (`None` on success)
//! so the caller can render a single "what happened" block at the end.
//! The report implements both [`fmt::Display`] (for the terminal
//! transcript) and [`serde::Serialize`] (for `--json`), so the
//! `synchronize` command path just forwards the value to
//! [`pimalaya_cli::printer::Printer::out`] and lets the printer pick
//! the encoding.

use std::fmt;

use serde::Serialize;

use crate::{
    side::Side,
    sync::hunk::{EmailHunk, MailboxHunk},
};

#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    pub account: String,
    pub dry_run: bool,
    pub mailbox: PatchOutcome<MailboxHunk>,
    pub email: PatchOutcome<EmailHunk>,
    /// Content-key collisions detected while building the per-mailbox
    /// message map. The first envelope at each colliding key was kept
    /// (so the diff still applies to it); the rest were skipped for
    /// this sync and surface here so the user can fix the source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collisions: Vec<MessageCollision>,
}

/// One content-key collision group. `ids` lists every backend-native
/// id that hashed to the same bucket; the first id was kept in the
/// diff, the rest were skipped this sync.
#[derive(Debug, Serialize)]
pub struct MessageCollision {
    pub side: Side,
    pub mailbox: String,
    /// Shared `Message-ID:` value when every colliding envelope
    /// carried one. `None` means at least one envelope had no
    /// `Message-ID` and the legacy `(subject, date, from)` fallback
    /// collapsed them.
    pub message_id: Option<String>,
    pub ids: Vec<String>,
}

impl fmt::Display for MessageCollision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            mailbox,
            message_id,
            ids,
        } = self;
        // The first id is the one that survived in the diff (kept the
        // first time we saw the content key); everything after it was
        // skipped this sync to avoid syncing two messages with the
        // same Message-ID twice.
        let kept = ids.first().map(String::as_str).unwrap_or("?");
        let skipped = ids
            .iter()
            .skip(1)
            .map(|id| format!("`{id}`"))
            .collect::<Vec<_>>()
            .join(", ");
        match message_id {
            Some(mid) => write!(
                f,
                "skip {skipped} on {side} `{mailbox}`: same Message-ID `{mid}` as `{kept}`"
            ),
            None => write!(
                f,
                "skip {skipped} on {side} `{mailbox}`: same subject/date/sender as `{kept}` (no Message-ID header)"
            ),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PatchOutcome<H> {
    pub patch: Vec<PatchEntry<H>>,
}

impl<H> Default for PatchOutcome<H> {
    fn default() -> Self {
        Self { patch: Vec::new() }
    }
}

#[derive(Debug, Serialize)]
pub struct PatchEntry<H> {
    pub hunk: H,
    /// Formatted error (`{e:#}`) when the hunk failed to apply,
    /// `None` on success. Stored as a string so the whole report is
    /// `Serialize`; `anyhow::Error` is not.
    pub error: Option<String>,
}

impl<H> PatchEntry<H> {
    pub fn new(hunk: H, error: Option<anyhow::Error>) -> Self {
        Self {
            hunk,
            error: error.map(|e| format!("{e:#}")),
        }
    }
}

impl fmt::Display for SyncReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f)?;

        let total = self.mailbox.patch.len() + self.email.patch.len();
        let mailbox_errors = self
            .mailbox
            .patch
            .iter()
            .filter(|e| e.error.is_some())
            .count();
        let email_errors = self
            .email
            .patch
            .iter()
            .filter(|e| e.error.is_some())
            .count();
        let errors = mailbox_errors + email_errors;
        let warnings = self.collisions.len();

        // Full listing first: every planned / applied hunk, plain.
        if !self.mailbox.patch.is_empty() {
            writeln!(f, "Mailbox patches ({n}):", n = self.mailbox.patch.len())?;
            for entry in &self.mailbox.patch {
                writeln!(f, " - {hunk}", hunk = entry.hunk)?;
            }
            writeln!(f)?;
        }

        if !self.email.patch.is_empty() {
            writeln!(f, "Message patches ({n}):", n = self.email.patch.len())?;
            for entry in &self.email.patch {
                writeln!(f, " - {hunk}", hunk = entry.hunk)?;
            }
            writeln!(f)?;
        }

        // Warnings: things the sync deliberately did not touch and
        // the user has to resolve manually (RFC violations, duplicate
        // Message-IDs). State on disk is untouched.
        if warnings > 0 {
            writeln!(f, "Warnings ({warnings}):")?;
            for c in &self.collisions {
                writeln!(f, " - {c}")?;
            }
            writeln!(f)?;
        }

        // Errors: things the sync tried and failed (real-mode only;
        // dry-run never carries any). Item state is uncertain on at
        // least one side; rerun the sync to retry.
        if errors > 0 {
            writeln!(f, "Errors ({errors}):")?;
            for entry in self.mailbox.patch.iter().filter(|e| e.error.is_some()) {
                writeln!(
                    f,
                    " - {hunk}: {err}",
                    hunk = entry.hunk,
                    err = entry.error.as_deref().unwrap_or_default(),
                )?;
            }
            for entry in self.email.patch.iter().filter(|e| e.error.is_some()) {
                writeln!(
                    f,
                    " - {hunk}: {err}",
                    hunk = entry.hunk,
                    err = entry.error.as_deref().unwrap_or_default(),
                )?;
            }
            writeln!(f)?;
        }

        let account = &self.account;
        match (total, errors, warnings, self.dry_run) {
            (0, 0, 0, _) => write!(f, "Account `{account}` is already in sync"),
            (0, 0, w, _) => write!(f, "Account `{account}` is already in sync ({w} warnings)"),
            (n, 0, 0, true) => write!(f, "Account `{account}` would apply {n} hunks"),
            (n, 0, w, true) => write!(
                f,
                "Account `{account}` would apply {n} hunks ({w} warnings)"
            ),
            (n, e, 0, true) => write!(
                f,
                "Account `{account}` would apply {n} hunks ({e} would fail)"
            ),
            (n, e, w, true) => write!(
                f,
                "Account `{account}` would apply {n} hunks ({e} would fail, {w} warnings)"
            ),
            (n, 0, 0, false) => write!(f, "Account `{account}` synchronized: {n} hunks"),
            (n, 0, w, false) => write!(
                f,
                "Account `{account}` synchronized: {n} hunks, {w} warnings"
            ),
            (n, e, 0, false) => write!(
                f,
                "Account `{account}` partially synchronized: {n} hunks, {e} errors"
            ),
            (n, e, w, false) => write!(
                f,
                "Account `{account}` partially synchronized: {n} hunks, {e} errors, {w} warnings"
            ),
        }
    }
}
