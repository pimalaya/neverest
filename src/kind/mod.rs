//! The per-kind seam: everything about a synced item that depends on
//! *which* media type it is.
//!
//! io-replica, io-pimdir and the [`crate::client`] seam are all
//! kind-agnostic; exactly three things are not, and they live here:
//!
//! - the **link id**, an item's stable cross-collection identity (a
//!   `Message-ID`, a vCard or iCalendar `UID`);
//! - the **summary**, the `v:1` JSON blob a reader renders a list from
//!   without fetching a body (pimdir SPEC Annex A);
//! - the **sort key**, the item's place in its collection's natural order
//!   (newest first for mail, A to Z for cards), which the store orders a
//!   page by and never parses out of the summary (pimdir SPEC §9.3).
//!
//! All three are derived from a raw body by [`Kind::parse_body`], the single
//! dispatch point the sync goes through. A kind that also has a cheap
//! server-side summary tier (mail's IMAP `ENVELOPE`) additionally
//! implements [`Kind::parse_summary`], so the `Meta` tier resolves without
//! a body. **A kind that implements both MUST make them agree
//! byte-for-byte** — see [`mail`] for what happens when they do not.
//!
//! Deliberately an enum rather than a trait: the set of kinds is closed
//! and small, there is one dispatch point, and it mirrors how
//! [`crate::client::Client`] already dispatches over its backends. A
//! trait would buy dynamic dispatch nobody needs.

pub mod mail;
#[cfg(feature = "carddav")]
pub mod vcard;

use io_replica::{
    placement::{ReplicaLinkId, ReplicaMeta, ReplicaSortKey},
    remote::ReplicaTier,
};

use crate::item::summary::ItemSummary;

/// The media types neverest can sync, one variant per kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// `message/rfc822`: mail, over IMAP or Microsoft Graph.
    Mail,
    /// `text/vcard`: contact cards, over CardDAV.
    #[cfg(feature = "carddav")]
    Vcard,
}

impl Kind {
    /// The kind a backend's [`media_type`](crate::client::Client::media_type)
    /// names, or `None` for a media type this build cannot sync.
    pub fn from_media_type(media_type: &str) -> Option<Self> {
        match media_type {
            "message/rfc822" => Some(Self::Mail),
            #[cfg(feature = "carddav")]
            "text/vcard" => Some(Self::Vcard),
            _ => None,
        }
    }

    /// The IANA media type, recorded as the pimdir collection's `kind`.
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Mail => "message/rfc822",
            #[cfg(feature = "carddav")]
            Self::Vcard => "text/vcard",
        }
    }

    /// The link id, `v:1` summary and sort key for a raw body of `size`
    /// octets — the `Full` tier's derivation, and the only one every kind
    /// implements.
    pub fn parse_body(self, raw: &[u8], size: u64) -> (ReplicaLinkId, ReplicaMeta, ReplicaSortKey) {
        match self {
            Self::Mail => mail::parse_body(raw, size),
            #[cfg(feature = "carddav")]
            Self::Vcard => vcard::parse_body(raw, size),
        }
    }

    /// The part of a link id a backend can address a new item by: the
    /// `Message-ID` an IMAP server without UIDPLUS needs to recover the UID
    /// it assigned, the `UID` a DAV backend builds the new href from.
    ///
    /// A link id *is* that identity, pimdir SPEC Annex A prepending nothing
    /// to it, so the hint is the id itself. `None` only for the kind's own
    /// fallback (mail's `alt:`, a card's `hash:`), which the server has never
    /// heard of: those are the one case a prefix marks, and a real
    /// `Message-ID` or `UID` cannot be mistaken for one, RFC 5322 `atext`
    /// admitting no colon before the `@`.
    pub fn link_hint(self, link_id: &ReplicaLinkId) -> Option<&str> {
        let fallback = match self {
            Self::Mail => "alt:",
            #[cfg(feature = "carddav")]
            Self::Vcard => "hash:",
        };

        (!link_id.0.starts_with(fallback)).then_some(link_id.0.as_str())
    }

    /// The tier a freshly probed item is raised to so its link id and summary
    /// resolve: `Meta` where the backend has a cheap server-side summary
    /// (mail's IMAP `ENVELOPE`), `Full` where only the body carries the
    /// identity, which is every kind [`parse_summary`](Self::parse_summary)
    /// answers `None` for.
    pub fn probe_tier(self) -> ReplicaTier {
        match self {
            Self::Mail => ReplicaTier::Meta,
            #[cfg(feature = "carddav")]
            Self::Vcard => ReplicaTier::Full,
        }
    }

    /// The link id, `v:1` summary and sort key from a server-side summary,
    /// for a kind whose backend offers a cheap `Meta` tier (mail's IMAP
    /// `ENVELOPE`).
    ///
    /// `None` for a kind with no such tier: a DAV `sync-collection` report
    /// returns hrefs and ETags but no `UID`, so a DAV item can only resolve
    /// from its body and goes straight to `Full`.
    pub fn parse_summary(
        self,
        summary: &ItemSummary,
    ) -> Option<(ReplicaLinkId, ReplicaMeta, ReplicaSortKey)> {
        match self {
            Self::Mail => Some(mail::parse_summary(summary)),
            #[cfg(feature = "carddav")]
            Self::Vcard => None,
        }
    }
}
