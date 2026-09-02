//! # Collection
//!
//! The collection shape shared across all protocols and kinds.

use serde::{Deserialize, Serialize};

/// A collection of items: a mailbox, an address book, a calendar.
///
/// Strict least-common-denominator: protocol-specific data (IMAP
/// delimiter and SPECIAL-USE attributes, JMAP rights, DAV privileges, …)
/// is intentionally absent.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    /// Backend-specific identifier: a JMAP id, a DAV href, the name on IMAP.
    pub id: String,

    /// Human-readable collection name.
    pub name: String,

    /// Total number of items; `None` when not asked or not cheap to answer.
    #[serde(default)]
    pub total: Option<u64>,

    /// Number of unread items, when the caller requested counts.
    ///
    /// `None` when not asked, not cheap to answer, or the kind has no
    /// notion of unread (every kind but mail).
    #[serde(default)]
    pub unread: Option<u64>,
}
