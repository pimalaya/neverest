//! The per-item summary the sync caches so a reader can render a list
//! without fetching a body (pimdir SPEC Annex A's `meta`).
//!
//! Still mail-shaped: the fields below are RFC 5322's. The kind seam
//! (change `generic-pim-sync`, phase 2) replaces this with one summary
//! per media type.

use std::collections::BTreeSet;

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

use crate::item::{address::Address, flag::Flag};

/// Lightweight summary of a message: enough to display in a list
/// without fetching the full body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ItemSummary {
    /// Backend-specific identifier of the message.
    ///
    /// IMAP UID or JMAP email ID.
    pub id: String,

    /// `Message-ID:` header value (RFC 5322 §3.6.4), `None` when the
    /// header is missing or the backend did not surface it. Stable
    /// across every backend that stores the message.
    #[serde(default)]
    pub message_id: Option<String>,

    /// `In-Reply-To:` header value (RFC 5322 §3.6.4), the message(s)
    /// this one replies to, empty when the header is missing or the
    /// backend did not surface it.
    ///
    /// A list because the grammar is `1*msg-id`, and normalised like
    /// [`message_id`](Self::message_id), so a reply and its parent
    /// compare byte-for-byte.
    #[serde(default)]
    pub in_reply_to: Vec<String>,

    /// Flags set on the message. Stored as a sorted set since wire
    /// order is not meaningful and duplicates are nonsensical.
    #[serde(default)]
    pub flags: BTreeSet<Flag>,

    /// Subject header value.
    #[serde(default)]
    pub subject: String,

    /// Sender(s).
    #[serde(default)]
    pub from: Vec<Address>,

    /// Primary recipient(s).
    #[serde(default)]
    pub to: Vec<Address>,

    /// Author-claimed send time, taken from the `Date:` header (IMAP
    /// `ENVELOPE.date`, JMAP `sentAt`).
    /// `None` when the header is missing or unparseable.
    #[serde(default)]
    pub date: Option<DateTime<FixedOffset>>,

    /// Size of the raw RFC 5322 message in bytes.
    #[serde(default)]
    pub size: u64,

    /// Whether the message has at least one attachment, when the caller
    /// opted in. `None` when not requested or when detection is not
    /// implemented for the active backend.
    #[serde(default)]
    pub has_attachment: Option<bool>,
}

/// Splits a raw `In-Reply-To:` value into its bare message ids.
///
/// RFC 5322 §3.6.4 gives the field as `1*msg-id`, and a backend hands
/// the whole value over as one string (the IMAP `ENVELOPE`), so the ids
/// are read off the angle brackets that delimit them. A value carrying
/// none is split on whitespace instead. Each id is normalised like
/// [`normalize_message_id`].
#[cfg_attr(not(any(feature = "imap", feature = "msgraph")), allow(dead_code))]
pub fn parse_message_ids(raw: &str) -> Vec<String> {
    if raw.contains('<') {
        return raw
            .split('<')
            .filter_map(|rest| rest.split_once('>'))
            .filter_map(|(id, _)| normalize_message_id(id))
            .collect();
    }

    raw.split_whitespace()
        .filter_map(normalize_message_id)
        .collect()
}

/// Strips RFC 5322 `msg-id` wrappers from the raw `Message-ID:` value
/// so every backend's [`ItemSummary::message_id`] is comparable
/// byte-for-byte. Whitespace and a single pair of angle brackets are
/// removed; an empty result becomes `None`. Only the backends parsing a
/// remote's envelope call it.
#[cfg_attr(not(any(feature = "imap", feature = "msgraph")), allow(dead_code))]
pub fn normalize_message_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let inner = trimmed
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(trimmed)
        .trim();

    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
    }
}
