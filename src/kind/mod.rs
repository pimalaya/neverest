//! The per-kind seam: everything about a synced item that depends on
//! *which* media type it is.
//!
//! io-replica, io-pimdir and the [`crate::client`] seam are all
//! kind-agnostic; exactly four things are not, and they live here:
//!
//! - the **link id**, an item's stable cross-collection identity (a
//!   `Message-ID`, a vCard or iCalendar `UID`);
//! - the **summary**, the `v:1` JSON blob a reader renders a list from
//!   without fetching a body (pimdir SPEC Annex A);
//! - the **sort key**, the item's place in its collection's natural order
//!   (newest first for mail, A to Z for cards, chronological for calendar
//!   items), which the store orders a page by and never parses out of the
//!   summary (pimdir SPEC §9.3);
//! - the **merge**, the three-way reconciliation of a content conflict's
//!   base, local and remote bodies, which lives in [`merge`] and is what
//!   keeps io-replica free of every format.
//!
//! The first three are derived from a raw body by [`Kind::parse_body`], the single
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

#[cfg(feature = "dav")]
pub mod ical;
pub mod mail;
pub mod merge;
#[cfg(feature = "dav")]
pub mod vcard;

use anyhow::{Result, bail};
use io_replica::{
    placement::{ReplicaLinkId, ReplicaMeta, ReplicaSortKey},
    remote::ReplicaTier,
};

use crate::item::summary::ItemSummary;

/// The prefix a minted link id carries, followed by the identity hint
/// (pimdir SPEC §9).
const MINT_PREFIX: &str = "dup:";

/// What separates a minted key's hint from the handle it was minted on
/// (pimdir SPEC §9).
const MINT_SEPARATOR: char = '#';

/// A link id read as its parts by [`Kind::split_link_id`].
///
/// Both parts are absent for a key the item's content never stated (a kind
/// fallback with nothing minted onto it), which is the shape a write has no
/// identity to offer a server for.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LinkId<'a> {
    /// The identity the item states and a backend can address it by: the
    /// `Message-ID` an IMAP server without UIDPLUS needs to recover the UID
    /// it assigned, the `UID` a DAV backend builds the new href from. `None`
    /// for the kind's own fallback, which no server has heard of.
    pub hint: Option<&'a str>,
    /// The handle the key was minted on, for a second copy of an identity the
    /// collection holds twice. `None` for an ordinary key. It is what a name
    /// derived from the identity must carry to stay distinct from the twin's.
    pub mint: Option<&'a str>,
}

/// The media types neverest can sync, one variant per kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// `message/rfc822`: mail, over IMAP or Microsoft Graph.
    Mail,
    /// `text/vcard`: contact cards, over CardDAV.
    #[cfg(feature = "dav")]
    Vcard,
    /// `text/calendar`: calendar object resources, over CalDAV.
    #[cfg(feature = "dav")]
    Ical,
}

impl Kind {
    /// The kind a backend's [`media_type`](crate::client::Client::media_type)
    /// names, or `None` for a media type this build cannot sync.
    pub fn from_media_type(media_type: &str) -> Option<Self> {
        match media_type {
            "message/rfc822" => Some(Self::Mail),
            #[cfg(feature = "dav")]
            "text/vcard" => Some(Self::Vcard),
            #[cfg(feature = "dav")]
            "text/calendar" => Some(Self::Ical),
            _ => None,
        }
    }

    /// The IANA media type, recorded as the pimdir collection's `kind`.
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Mail => "message/rfc822",
            #[cfg(feature = "dav")]
            Self::Vcard => "text/vcard",
            #[cfg(feature = "dav")]
            Self::Ical => "text/calendar",
        }
    }

    /// The extension a body of this kind is exported under, so a merger a
    /// person configured is handed files it recognises rather than four
    /// nameless ones.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mail => "eml",
            #[cfg(feature = "dav")]
            Self::Vcard => "vcf",
            #[cfg(feature = "dav")]
            Self::Ical => "ics",
        }
    }

    /// The link id, `v:1` summary and sort key for a raw body of `size`
    /// octets — the `Full` tier's derivation, and the only one every kind
    /// implements.
    pub fn parse_body(self, raw: &[u8], size: u64) -> (ReplicaLinkId, ReplicaMeta, ReplicaSortKey) {
        match self {
            Self::Mail => mail::parse_body(raw, size),
            #[cfg(feature = "dav")]
            Self::Vcard => vcard::parse_body(raw, size),
            #[cfg(feature = "dav")]
            Self::Ical => ical::parse_body(raw, size),
        }
    }

    /// Refuses a body that is not one of this kind, or not one of *this*
    /// item.
    ///
    /// A settled conflict body is the one body reaching the store that
    /// nothing here derived: a person wrote it, or the merger they named
    /// did. Two things are asked of it. It has to read as the kind the
    /// collection declares, which is what keeps a half-written template, a
    /// tool that wrote its error message to the output path, or a merger
    /// that crashed mid-write from replacing a contact with something that
    /// is not one. And it has to keep the identity the item is bound by: a
    /// body stating another `UID`, or none, is a resolution of some other
    /// item, and taking it leaves the store holding a row whose link id has
    /// nothing to do with its content.
    ///
    /// The reading is the kind's own scanner rather than the merge's parser,
    /// so a build without the `merge` cargo feature, where an interactive
    /// resolution is the only way a divergence is ever settled, is guarded
    /// the same way.
    pub fn validate_body(self, body: &[u8], link_id: &ReplicaLinkId) -> Result<()> {
        let Some(component) = self.component() else {
            bail!("Mail bodies are immutable, so no body settles a message");
        };

        if !wrapped_in(body, component) {
            bail!(
                "A {} body opens with BEGIN:{component} and closes with END:{component}",
                self.media_type()
            );
        }

        let (derived, _, _) = self.parse_body(body, body.len() as u64);
        let stated = self.split_link_id(&derived).hint;
        let bound = self.split_link_id(link_id).hint;

        match (bound, stated) {
            (bound, stated) if bound == stated => Ok(()),
            (Some(bound), Some(stated)) => {
                bail!("A settled body keeps the item's UID {bound}, and this one states {stated}")
            }
            (Some(bound), None) => {
                bail!("A settled body keeps the item's UID {bound}, and this one states none")
            }
            (None, Some(stated)) => bail!(
                "The item states no UID of its own, and a settled body cannot give it {stated}"
            ),
            (None, None) => unreachable!("two absent hints compare equal"),
        }
    }

    /// The component a body of this kind is wrapped in (RFC 6350 §6.1.1, RFC
    /// 5545 §3.4), or `None` for a kind whose bodies are opaque to every
    /// reader here.
    fn component(self) -> Option<&'static str> {
        match self {
            Self::Mail => None,
            #[cfg(feature = "dav")]
            Self::Vcard => Some("VCARD"),
            #[cfg(feature = "dav")]
            Self::Ical => Some("VCALENDAR"),
        }
    }

    /// Splits a link id into the two things a write needs from it: the
    /// identity a backend can address the item by, and what tells this copy
    /// from the one already holding that identity.
    ///
    /// **The one legitimate place a link id is parsed.** The store never
    /// parses one, and neither does anything else here: a key is opaque to
    /// every reader (pimdir SPEC §9), and the single exception is the write
    /// side, which has to hand a server an identity it can act on. Every
    /// backend therefore takes its hint and its mint from here rather than
    /// reading the string itself.
    ///
    /// An ordinary key *is* the identity, pimdir SPEC Annex A prepending
    /// nothing to it. A minted key (`dup:<hint>#<handle>`, SPEC §9) is the
    /// second copy of an identity one collection holds twice: its hint is the
    /// identity both copies genuinely carry, so an append resolves against the
    /// server by it, and its mint is the handle it was minted from, which is
    /// what keeps the two copies apart in anything named after them.
    pub fn split_link_id<'l>(self, link_id: &'l ReplicaLinkId) -> LinkId<'l> {
        let Some(minted) = link_id.0.strip_prefix(MINT_PREFIX) else {
            return LinkId {
                hint: self.hint(&link_id.0),
                mint: None,
            };
        };

        // The last separator, not the first: a `Message-ID` may legally carry
        // a `#` (RFC 5322 `atext` admits it) and a handle addressed as one
        // path segment or a UID may not.
        let (hint, mint) = minted.rsplit_once(MINT_SEPARATOR).unwrap_or(("", minted));

        LinkId {
            hint: self.hint(hint),
            mint: Some(mint),
        }
    }

    /// The identity in a key, or `None` for the kind's own fallback (mail's
    /// `alt:`, a DAV item's `hash:`), which the server has never heard of.
    ///
    /// Those are the one case a prefix marks, and a real `Message-ID` or `UID`
    /// cannot be mistaken for one, RFC 5322 `atext` admitting no colon before
    /// the `@`.
    fn hint(self, key: &str) -> Option<&str> {
        let fallback = match self {
            Self::Mail => "alt:",
            #[cfg(feature = "dav")]
            Self::Vcard | Self::Ical => "hash:",
        };

        (!key.is_empty() && !key.starts_with(fallback)).then_some(key)
    }

    /// The tier a freshly probed item is raised to so its link id and summary
    /// resolve: `Meta` where the backend has a cheap server-side summary
    /// (mail's IMAP `ENVELOPE`), `Full` where only the body carries the
    /// identity, which is every kind [`parse_summary`](Self::parse_summary)
    /// answers `None` for.
    pub fn probe_tier(self) -> ReplicaTier {
        match self {
            Self::Mail => ReplicaTier::Meta,
            #[cfg(feature = "dav")]
            Self::Vcard | Self::Ical => ReplicaTier::Full,
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
            #[cfg(feature = "dav")]
            Self::Vcard | Self::Ical => None,
        }
    }
}

/// Whether `body`'s first and last content lines are the `BEGIN` and the
/// `END` of `component`.
///
/// Blank lines are skipped at both ends, a trailing line terminator being
/// optional in what the store holds, and the comparison is ASCII
/// case-insensitive because both formats spell their delimiters that way.
fn wrapped_in(body: &[u8], component: &str) -> bool {
    let text = String::from_utf8_lossy(body);
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());

    let opens = lines
        .next()
        .is_some_and(|line| line.eq_ignore_ascii_case(&format!("BEGIN:{component}")));
    let closes = lines
        .next_back()
        .is_some_and(|line| line.eq_ignore_ascii_case(&format!("END:{component}")));

    opens && closes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ordinary_key_is_its_own_hint_and_mints_nothing() {
        let link = ReplicaLinkId::from("a@example.org");
        assert_eq!(
            Kind::Mail.split_link_id(&link),
            LinkId {
                hint: Some("a@example.org"),
                mint: None,
            },
        );
    }

    #[test]
    fn a_kind_fallback_offers_no_hint() {
        let link = ReplicaLinkId::from("alt:subject|date|from");
        assert_eq!(Kind::Mail.split_link_id(&link), LinkId::default());
    }

    /// The second copy of an identity keeps the identity, which is what an
    /// append resolves against, and gains the part that tells it from the
    /// copy holding that identity bare.
    #[test]
    fn a_minted_key_splits_into_the_shared_identity_and_the_copys_own_part() {
        let link = ReplicaLinkId::from("dup:a@example.org#146");
        assert_eq!(
            Kind::Mail.split_link_id(&link),
            LinkId {
                hint: Some("a@example.org"),
                mint: Some("146"),
            },
        );
    }

    /// A `Message-ID` may carry a `#`, a handle addressed as one path segment
    /// may not, so the mint is what follows the last one.
    #[test]
    fn a_hint_carrying_the_separator_survives_the_split() {
        let link = ReplicaLinkId::from("dup:a#b@example.org#146");
        assert_eq!(
            Kind::Mail.split_link_id(&link),
            LinkId {
                hint: Some("a#b@example.org"),
                mint: Some("146"),
            },
        );
    }

    /// Two copies of a resource carrying no `UID` are minted over the kind's
    /// fallback, so the mint is the only part a write can name them by.
    #[test]
    #[cfg(feature = "dav")]
    fn a_mint_over_a_fallback_keeps_the_mint_and_no_hint() {
        let link = ReplicaLinkId::from("dup:hash:cbf29ce484222325#card-2.vcf");
        assert_eq!(
            Kind::Vcard.split_link_id(&link),
            LinkId {
                hint: None,
                mint: Some("card-2.vcf"),
            },
        );
    }

    /// The reported shape: one iCalendar `UID` under two hrefs, the second
    /// minted on the href it came from.
    #[test]
    #[cfg(feature = "dav")]
    fn a_minted_calendar_key_names_the_href_it_came_from() {
        let link = ReplicaLinkId::from("dup:event-1@google.com#event-1%2540google.com.ics");
        assert_eq!(
            Kind::Ical.split_link_id(&link),
            LinkId {
                hint: Some("event-1@google.com"),
                mint: Some("event-1%2540google.com.ics"),
            },
        );
    }
}
