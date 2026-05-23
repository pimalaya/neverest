//! Atomic units of sync work.
//!
//! Hunks are the smallest reversible operation the engine knows how
//! to apply: a single mailbox create/delete, a single message
//! copy/delete, or a flag mutation on one side. The diff produces a
//! flat list; the engine applies them in order and stamps any error
//! on the matching [`crate::sync::report::PatchOutcome`] entry.

use std::collections::BTreeSet;
use std::fmt;

use anyhow::Result;
use io_email::{client::EmailClientStd, flag::Flag};
use serde::Serialize;

use crate::side::Side;

/// Mailbox-level patch hunk. `Create` materializes a missing mailbox
/// on `side`; `Delete` removes one. The diff classifies a missing
/// mailbox as either add or delete by three-way against the cached
/// snapshot; per-side permissions (`mailbox.create` / `mailbox.delete`)
/// gate emission.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum MailboxHunk {
    Create { side: Side, mailbox: String },
    Delete { side: Side, mailbox: String },
}

impl MailboxHunk {
    /// Applies this hunk against the per-worker client pair. `Create`
    /// and `Delete` route to the side carried by the hunk variant via
    /// [`Side::client_mut`].
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

/// Envelope/message-level patch hunk.
///
/// Every variant carries `content_key`: the cross-side identifier the
/// diff used to align messages. The engine reuses it post-apply to
/// mutate the per-mailbox snapshot in place (insert on Copy, remove
/// on Delete, update flags on AddFlags/RemoveFlags) so the next sync's
/// diff sees the just-applied state as the baseline. `content_key`
/// is `#[serde(skip)]` to keep the JSON report shape stable.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum EmailHunk {
    /// Copy the message identified by `source_id` on `source_side`
    /// over to `target_side`. The engine fetches the raw RFC 5322
    /// bytes via `get_message` then re-appends them via `add_message`
    /// preserving the source flags. `apply` returns the backend-
    /// assigned id of the new message on the target side.
    Copy {
        source_side: Side,
        target_side: Side,
        mailbox: String,
        source_id: String,
        flags: BTreeSet<Flag>,
        #[serde(skip)]
        content_key: u64,
    },
    /// Add the given flags on `side`'s copy of the message. Emitted
    /// when a flag is present on the opposite side and absent both on
    /// `side`'s current envelope and on `side`'s prior snapshot
    /// (i.e. the opposite side added it).
    AddFlags {
        side: Side,
        mailbox: String,
        id: String,
        flags: BTreeSet<Flag>,
        #[serde(skip)]
        content_key: u64,
    },
    /// Remove the given flags from `side`'s copy of the message.
    /// Emitted when a flag is absent on the opposite side and present
    /// both on `side`'s current envelope and on the opposite side's
    /// prior snapshot (i.e. the opposite side removed it). Without a
    /// prior snapshot the diff treats the divergence as an add on the
    /// side that has it; removal only fires once a baseline exists.
    RemoveFlags {
        side: Side,
        mailbox: String,
        id: String,
        flags: BTreeSet<Flag>,
        #[serde(skip)]
        content_key: u64,
    },
    /// Delete `side`'s copy of the message. The engine marks `\Deleted`
    /// via `delete_flags` and relies on the backend's own expunge
    /// semantics (IMAP needs an explicit `EXPUNGE`; Maildir removes
    /// the file on flag flip; JMAP applies on the next `Email/set`).
    Delete {
        side: Side,
        mailbox: String,
        id: String,
        #[serde(skip)]
        content_key: u64,
    },
}

impl EmailHunk {
    /// Applies this hunk against the per-worker client pair. Returns
    /// the backend-assigned id of the newly-stored message for `Copy`
    /// (so the engine can record it in the snapshot at `content_key`);
    /// `None` for the other variants.
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
                let (source, target) = Side::pair_mut(*source_side, *target_side, left, right);
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
/// `[\seen, \flagged]`. Used by [`EmailHunk`]'s `Display` so terminal
/// output stays readable instead of leaking the derived `Debug` shape
/// of the underlying `BTreeSet`.
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
