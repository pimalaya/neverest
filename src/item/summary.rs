//! # Item summary
//!
//! The per-item summary the sync caches so a reader can render a list
//! without fetching a body (pimdir SPEC Annex A's `meta`). Still
//! mail-shaped: phase 2 of the kind seam replaces it with one summary per
//! media type.

use std::collections::BTreeSet;

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

use crate::item::{address::Address, flag::Flag};

/// Enough of a message to render a list entry without fetching its body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSummary {
    /// Backend-specific identifier: an IMAP UID, a JMAP email id.
    pub id: String,

    /// `Message-ID:` (RFC 5322 §3.6.4), `None` when missing or unsurfaced.
    ///
    /// Normalised, so it is stable across every backend that stores the
    /// message.
    #[serde(default)]
    pub message_id: Option<String>,

    /// `In-Reply-To:` (RFC 5322 §3.6.4), empty when missing or unsurfaced.
    ///
    /// A list because the grammar is `1*msg-id`, normalised like
    /// [`message_id`](Self::message_id) so a reply and its parent compare
    /// byte-for-byte.
    #[serde(default)]
    pub in_reply_to: Vec<String>,

    /// Flags set on the message, a sorted set: wire order means nothing.
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

    /// Author-claimed send time from the `Date:` header, `None` when the
    /// header is missing or unparseable.
    #[serde(default)]
    pub date: Option<DateTime<FixedOffset>>,

    /// Size of the raw RFC 5322 message in bytes.
    #[serde(default)]
    pub size: u64,

    /// Whether the message has an attachment; `None` when not requested
    /// or not detectable on the active backend.
    #[serde(default)]
    pub has_attachment: Option<bool>,
}

/// Splits a raw `In-Reply-To:` value into its bare message ids.
///
/// RFC 5322 §3.6.4 gives the field as `1*msg-id` in one string, so the ids
/// are read off their angle brackets, or off whitespace when there are
/// none. Each is normalised like [`normalize_message_id`].
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

/// Strips the RFC 5322 `msg-id` wrappers from a raw `Message-ID:` value.
///
/// Whitespace and a single pair of angle brackets are removed, an empty
/// result becoming `None`, so that every backend's
/// [`ItemSummary::message_id`] is comparable byte-for-byte.
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
