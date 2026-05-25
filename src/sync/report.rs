// This file is part of Neverest, a CLI to synchronize emails.
//
// Copyright (C) 2024-2026  soywod <pimalaya.org@posteo.net>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! End-of-run summary returned by the sync engine; implements `Display`
//! for the terminal and `Serialize` for `--json`.

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
    /// Content-key collisions surfaced this sync (first envelope kept,
    /// rest skipped).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collisions: Vec<MessageCollision>,
}

/// One content-key collision group; first id in `ids` is the kept one.
#[derive(Debug, Serialize)]
pub struct MessageCollision {
    pub side: Side,
    pub mailbox: String,
    /// Shared `Message-ID:` when every envelope carried one; `None`
    /// when the legacy `(subject, date, from)` fallback collapsed
    /// envelopes without a header.
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
    /// Formatted apply error (`{e:#}`); `None` on success.
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

        if warnings > 0 {
            writeln!(f, "Warnings ({warnings}):")?;
            for c in &self.collisions {
                writeln!(f, " - {c}")?;
            }
            writeln!(f)?;
        }

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
