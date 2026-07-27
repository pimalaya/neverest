//! Collection shared across all protocols and kinds.

use serde::{Deserialize, Serialize};

/// A collection of items: a mailbox, an address book, a calendar.
///
/// Strict least-common-denominator shape: only fields that are
/// first-class in every protocol the sync targets. Protocol-specific
/// data (IMAP delimiter and SPECIAL-USE attributes, JMAP role and
/// rights, DAV privileges, …) is intentionally absent.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Collection {
    /// Backend-specific identifier.
    ///
    /// JMAP exposes a real opaque ID and DAV a href; for IMAP this is
    /// the same as [`Self::name`]. Use this when issuing follow-up
    /// commands that refer to the collection.
    pub id: String,

    /// Human-readable collection name.
    pub name: String,

    /// Total number of items, when the caller requested counts.
    /// `None` when the backend was not asked or cannot answer cheaply.
    #[serde(default)]
    pub total: Option<u64>,

    /// Number of unread items, when the caller requested counts.
    /// `None` when the backend was not asked, cannot answer cheaply, or
    /// has no notion of unread (every kind but mail).
    #[serde(default)]
    pub unread: Option<u64>,
}
