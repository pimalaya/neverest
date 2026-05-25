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

//! Atomic sync work units: mailbox / message / flag hunks emitted by
//! the diff and applied by the worker pool.

use std::collections::BTreeSet;
use std::fmt;

use anyhow::Result;
use io_email::{client::EmailClientStd, flag::Flag};
use serde::Serialize;

use crate::side::Side;

/// Mailbox-level patch hunk: create or delete a mailbox on one side.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MailboxHunk {
    Create { side: Side, mailbox: String },
    Delete { side: Side, mailbox: String },
}

impl MailboxHunk {
    /// Applies the hunk via the side's client.
    pub fn apply(&self, left: &mut EmailClientStd, right: &mut EmailClientStd) -> Result<()> {
        match self {
            Self::Create { side, mailbox } => {
                side.client_mut(left, right).create_mailbox(mailbox)?;
            }
            Self::Delete { side, mailbox } => {
                side.client_mut(left, right).delete_mailbox(mailbox)?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for MailboxHunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create { side, mailbox } => write!(f, "create mailbox `{mailbox}` on {side}"),
            Self::Delete { side, mailbox } => write!(f, "delete mailbox `{mailbox}` on {side}"),
        }
    }
}

/// Message-level patch hunk; `content_key` is the cross-side alignment
/// key, skipped from JSON to keep the report shape stable.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum EmailHunk {
    /// Copy a message from `source_side` to `target_side`; `apply`
    /// returns the new backend-assigned id on the target side.
    Copy {
        source_side: Side,
        target_side: Side,
        mailbox: String,
        source_id: String,
        flags: BTreeSet<Flag>,
        #[serde(skip)]
        content_key: u64,
    },
    /// Add `flags` on `side`'s copy of the message.
    AddFlags {
        side: Side,
        mailbox: String,
        id: String,
        flags: BTreeSet<Flag>,
        #[serde(skip)]
        content_key: u64,
    },
    /// Remove `flags` from `side`'s copy of the message.
    RemoveFlags {
        side: Side,
        mailbox: String,
        id: String,
        flags: BTreeSet<Flag>,
        #[serde(skip)]
        content_key: u64,
    },
    /// Delete `side`'s copy of the message via `delete_message`.
    Delete {
        side: Side,
        mailbox: String,
        id: String,
        #[serde(skip)]
        content_key: u64,
    },
}

impl EmailHunk {
    /// Applies the hunk; returns `Some(new_id)` for a successful
    /// `Copy` and `None` for the other variants.
    pub fn apply(
        &self,
        left: &mut EmailClientStd,
        right: &mut EmailClientStd,
    ) -> Result<Option<String>> {
        match self {
            Self::Copy {
                source_side,
                target_side,
                mailbox,
                source_id,
                flags,
                ..
            } => {
                let (source, target) = Side::pair_mut(*source_side, *target_side, left, right)?;
                let raw = source.get_message(mailbox, source_id)?;
                let flag_list: Vec<Flag> = flags.iter().cloned().collect();
                let target_id = target.add_message(mailbox, &flag_list, raw)?;
                Ok(Some(target_id))
            }
            Self::AddFlags {
                side,
                mailbox,
                id,
                flags,
                ..
            } => {
                let flag_list: Vec<Flag> = flags.iter().cloned().collect();
                side.client_mut(left, right)
                    .add_flags(mailbox, &[id.as_str()], &flag_list)?;
                Ok(None)
            }
            Self::RemoveFlags {
                side,
                mailbox,
                id,
                flags,
                ..
            } => {
                let flag_list: Vec<Flag> = flags.iter().cloned().collect();
                side.client_mut(left, right)
                    .delete_flags(mailbox, &[id.as_str()], &flag_list)?;
                Ok(None)
            }
            Self::Delete {
                side, mailbox, id, ..
            } => {
                side.client_mut(left, right).delete_message(mailbox, id)?;
                Ok(None)
            }
        }
    }
}

impl fmt::Display for EmailHunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Copy {
                source_side,
                target_side,
                mailbox,
                source_id,
                ..
            } => write!(
                f,
                "copy message `{source_id}` in `{mailbox}` from {source_side} to {target_side}"
            ),
            Self::AddFlags {
                side,
                mailbox,
                id,
                flags,
                ..
            } => write!(
                f,
                "add {flags} to message `{id}` in `{mailbox}` on {side}",
                flags = format_flag_list(flags),
            ),
            Self::RemoveFlags {
                side,
                mailbox,
                id,
                flags,
                ..
            } => write!(
                f,
                "remove {flags} from message `{id}` in `{mailbox}` on {side}",
                flags = format_flag_list(flags),
            ),
            Self::Delete {
                side, mailbox, id, ..
            } => {
                write!(f, "delete message `{id}` in `{mailbox}` on {side}")
            }
        }
    }
}

/// Lowercase comma-joined flag list wrapped in brackets, e.g.
/// `[\seen, \flagged]`.
fn format_flag_list(flags: &BTreeSet<Flag>) -> String {
    let mut out = String::from("[");
    let mut first = true;
    for flag in flags {
        if !first {
            out.push_str(", ");
        }
        first = false;
        for ch in flag.raw().chars() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        }
    }
    out.push(']');
    out
}
