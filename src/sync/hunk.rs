//! Atomic units of sync work.
//!
//! Hunks are the smallest reversible operation the engine knows how
//! to apply: a single mailbox create/delete, a single message
//! copy/delete, or a flag mutation on one side. The diff produces a
//! flat list; the engine applies them in order and stamps any error
//! on the matching [`crate::sync::report::PatchOutcome`] entry.

use std::collections::BTreeSet;
use std::fmt;

use io_email::flag::Flag;

use crate::side::Side;

/// Mailbox-level patch hunk. Only creates today; deletes will land
/// once `io_email::client::EmailClientStd` grows a delete-mailbox op
/// and the safety story around it is settled.
#[derive(Clone, Debug)]
pub enum MailboxHunk {
    Create { side: Side, mailbox: String },
}

impl fmt::Display for MailboxHunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create { side, mailbox } => write!(f, "create mailbox `{mailbox}` on {side}"),
        }
    }
}

/// Envelope/message-level patch hunk.
#[derive(Clone, Debug)]
pub enum EmailHunk {
    /// Copy the message identified by `source_id` on `source_side`
    /// over to `target_side`. The engine fetches the raw RFC 5322
    /// bytes via `get_message` then re-appends them via `add_message`
    /// preserving the source flags.
    Copy {
        source_side: Side,
        target_side: Side,
        mailbox: String,
        source_id: String,
        flags: BTreeSet<Flag>,
    },
    /// Add the given flags on `side`'s copy of the message. Sync
    /// only ever propagates the union of both sides' flag sets, so
    /// flag removals are deliberately not modeled here.
    AddFlags {
        side: Side,
        mailbox: String,
        id: String,
        flags: BTreeSet<Flag>,
    },
    /// Delete `side`'s copy of the message. The engine marks `\Deleted`
    /// via `delete_flags` and relies on the backend's own expunge
    /// semantics (IMAP needs an explicit `EXPUNGE`; Maildir removes
    /// the file on flag flip; JMAP applies on the next `Email/set`).
    Delete {
        side: Side,
        mailbox: String,
        id: String,
    },
}

impl EmailHunk {
    pub fn mailbox(&self) -> &str {
        match self {
            Self::Copy { mailbox, .. }
            | Self::AddFlags { mailbox, .. }
            | Self::Delete { mailbox, .. } => mailbox,
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
            } => write!(
                f,
                "add {flags:?} to message `{id}` in `{mailbox}` on {side}"
            ),
            Self::Delete { side, mailbox, id } => {
                write!(f, "delete message `{id}` in `{mailbox}` on {side}")
            }
        }
    }
}
