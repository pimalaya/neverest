//! # Kind
//!
//! The per-kind seam: everything about a synced item that depends on which
//! media type it is. io-pimdir and the [`crate::client`] seam are
//! kind-agnostic; exactly four things are not, and they live here.
//!
//! The link id is an item's stable cross-collection identity, the summary the
//! typed row a reader lists from without fetching a body (pimdir STORAGE
//! Annex A), the sort key its place in the collection's natural order (STORAGE
//! §9.3), and the merge the three-way reconciliation in [`merge`].
//!
//! The first three are io-pimdir's derivations, reached through
//! [`Kind::parse_body`]. A kind with a cheap server-side summary tier (mail's
//! IMAP `ENVELOPE`) also implements [`Kind::parse_summary`], and **a kind
//! implementing both MUST make them agree byte-for-byte**: see [`mail`].
//!
//! An enum rather than a trait: the set of kinds is closed and small, there
//! is one dispatch point, and a trait would buy dispatch nobody needs.

pub mod mail;
pub mod merge;

use anyhow::{Result, bail};
#[cfg(feature = "dav")]
use io_pimdir::summary::{calendar, contact};
use io_pimdir::{placement::PimdirLinkId, remote::PimdirTier, summary::PimdirDerivation};

use crate::item::summary::ItemSummary;

/// The prefix a minted link id carries, followed by the identity hint
/// (pimdir STORAGE §9).
const MINT_PREFIX: &str = "dup:";

/// What separates a minted key's hint from the handle it was minted on
/// (pimdir STORAGE §9).
const MINT_SEPARATOR: char = '#';

/// A link id read as its parts by [`Kind::split_link_id`].
///
/// Both parts are absent for a key the item's content never stated, which is
/// the shape a write has no identity to offer a server for.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LinkId<'a> {
    /// The identity the item states and a backend can address it by.
    ///
    /// The `Message-ID` an IMAP server without UIDPLUS recovers its UID from,
    /// the `UID` a DAV backend builds the new href from. `None` for the
    /// kind's own fallback, which no server has heard of.
    pub hint: Option<&'a str>,
    /// The handle the key was minted on, for a second copy of an identity one
    /// collection holds twice.
    ///
    /// `None` for an ordinary key. A name derived from the identity must
    /// carry it to stay distinct from the twin's.
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

    /// The extension a body of this kind is exported under, so a configured
    /// merger is handed files it recognises rather than four nameless ones.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mail => "eml",
            #[cfg(feature = "dav")]
            Self::Vcard => "vcf",
            #[cfg(feature = "dav")]
            Self::Ical => "ics",
        }
    }

    /// The link id, summary and sort key for a raw body of `size` octets: the
    /// `Full` tier's derivation, which every kind implements.
    ///
    /// `raw` is the prefix the stream captured, so a size read off it would be
    /// the prefix's; `size` is the whole body's, known from the stream.
    pub fn parse_body(self, raw: &[u8], size: u64) -> PimdirDerivation {
        match self {
            Self::Mail => mail::parse_body(raw, size),
            #[cfg(feature = "dav")]
            Self::Vcard => contact::derive(raw),
            #[cfg(feature = "dav")]
            Self::Ical => calendar::derive(raw),
        }
    }

    /// Refuses a body that is not of this kind, or not of *this* item.
    ///
    /// A settled body is the one body reaching the store that nothing here
    /// derived, so two things are asked of it: it reads as the kind the
    /// collection declares, and it keeps the identity the item is bound by.
    pub fn validate_body(self, body: &[u8], link_id: &PimdirLinkId) -> Result<()> {
        let Some(component) = self.component() else {
            bail!("Mail bodies are immutable, so no body settles a message");
        };

        if !wrapped_in(body, component) {
            bail!(
                "A {} body opens with BEGIN:{component} and closes with END:{component}",
                self.media_type()
            );
        }

        let derived = self.parse_body(body, body.len() as u64).link_id;
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
    /// 5545 §3.4), or `None` for a kind whose bodies are opaque here.
    fn component(self) -> Option<&'static str> {
        match self {
            Self::Mail => None,
            #[cfg(feature = "dav")]
            Self::Vcard => Some("VCARD"),
            #[cfg(feature = "dav")]
            Self::Ical => Some("VCALENDAR"),
        }
    }

    /// Splits a link id into the addressable identity and this copy's mint.
    ///
    /// **The one legitimate place a link id is parsed**: a key is opaque to
    /// every reader (pimdir STORAGE §9), only the write side handing a server
    /// an identity. The engine mints `dup:<hint>#<handle>`; this reads it back.
    pub fn split_link_id<'l>(self, link_id: &'l PimdirLinkId) -> LinkId<'l> {
        let Some(minted) = link_id.0.strip_prefix(MINT_PREFIX) else {
            return LinkId {
                hint: self.hint(&link_id.0),
                mint: None,
            };
        };

        // NOTE: the last separator, not the first: a `Message-ID` may legally
        // carry a `#` (RFC 5322 `atext`) and a handle may not.
        let (hint, mint) = minted.rsplit_once(MINT_SEPARATOR).unwrap_or(("", minted));

        LinkId {
            hint: self.hint(hint),
            mint: Some(mint),
        }
    }

    /// The identity in a key, or `None` for the kind's own fallback (`alt:`,
    /// `hash:`), which no server has heard of.
    ///
    /// The one case a prefix marks: a real `Message-ID` or `UID` cannot be
    /// mistaken for one, RFC 5322 `atext` admitting no colon before the `@`.
    fn hint(self, key: &str) -> Option<&str> {
        let fallback = match self {
            Self::Mail => "alt:",
            #[cfg(feature = "dav")]
            Self::Vcard | Self::Ical => "hash:",
        };

        (!key.is_empty() && !key.starts_with(fallback)).then_some(key)
    }

    /// The tier a freshly probed item is raised to so its link id and summary
    /// resolve: `Meta` where the backend has a cheap server-side summary,
    /// `Full` where only the body carries the identity.
    pub fn probe_tier(self) -> PimdirTier {
        match self {
            Self::Mail => PimdirTier::Meta,
            #[cfg(feature = "dav")]
            Self::Vcard | Self::Ical => PimdirTier::Full,
        }
    }

    /// The derivations from a server-side summary, for a kind with `Meta`.
    ///
    /// `None` for a kind with no such tier: a DAV `sync-collection` report
    /// returns hrefs and ETags but no `UID`, so a DAV item goes straight to
    /// `Full`.
    pub fn parse_summary(self, summary: &ItemSummary) -> Option<PimdirDerivation> {
        match self {
            Self::Mail => Some(mail::parse_summary(summary)),
            #[cfg(feature = "dav")]
            Self::Vcard | Self::Ical => None,
        }
    }
}

/// Whether `body` opens with `BEGIN:component` and closes with `END:`.
///
/// Blank lines are skipped at both ends, a trailing terminator being optional
/// in what the store holds, and the comparison is ASCII case-insensitive
/// because both formats spell their delimiters that way.
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
        let link = PimdirLinkId::from("a@example.org");
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
        let link = PimdirLinkId::from("alt:subject|date|from");
        assert_eq!(Kind::Mail.split_link_id(&link), LinkId::default());
    }

    /// The second copy keeps the identity an append resolves against, and
    /// gains the part telling it from the copy holding that identity bare.
    #[test]
    fn a_minted_key_splits_into_the_shared_identity_and_the_copys_own_part() {
        let link = PimdirLinkId::from("dup:a@example.org#146");
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
        let link = PimdirLinkId::from("dup:a#b@example.org#146");
        assert_eq!(
            Kind::Mail.split_link_id(&link),
            LinkId {
                hint: Some("a#b@example.org"),
                mint: Some("146"),
            },
        );
    }

    /// Two copies carrying no `UID` are minted over the kind's fallback, so
    /// the mint is the only part a write can name them by.
    #[test]
    #[cfg(feature = "dav")]
    fn a_mint_over_a_fallback_keeps_the_mint_and_no_hint() {
        let link = PimdirLinkId::from("dup:hash:cbf29ce484222325#card-2.vcf");
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
        let link = PimdirLinkId::from("dup:event-1@google.com#event-1%2540google.com.ics");
        assert_eq!(
            Kind::Ical.split_link_id(&link),
            LinkId {
                hint: Some("event-1@google.com"),
                mint: Some("event-1%2540google.com.ics"),
            },
        );
    }

    /// The keys the engine mints read back as the parts a write needs, so the
    /// mint the engine makes and the split this crate reads agree.
    #[test]
    fn a_key_the_engine_mints_splits_back_into_its_parts() {
        let minted = PimdirLinkId::from("a@example.org").minted(&"146".into());
        assert_eq!(
            Kind::Mail.split_link_id(&minted),
            LinkId {
                hint: Some("a@example.org"),
                mint: Some("146"),
            },
        );
    }

    /// A card body of this item settles it, a message body settles nothing.
    #[test]
    #[cfg(feature = "dav")]
    fn a_settled_body_is_read_as_the_kind_and_the_item_it_claims() {
        let bound = PimdirLinkId::from("uid:a");
        let card = b"BEGIN:VCARD\r\nVERSION:4.0\r\nUID:uid:a\r\nFN:Jane\r\nEND:VCARD\r\n";
        Kind::Vcard.validate_body(card, &bound).unwrap();

        let err = Kind::Vcard
            .validate_body(b"not a card", &bound)
            .unwrap_err()
            .to_string();
        assert!(err.contains("BEGIN:VCARD"), "{err}");

        let renamed = b"BEGIN:VCARD\r\nVERSION:4.0\r\nUID:uid:b\r\nEND:VCARD\r\n";
        let err = Kind::Vcard
            .validate_body(renamed, &bound)
            .unwrap_err()
            .to_string();
        assert!(err.contains("uid:b"), "{err}");

        assert!(Kind::Mail.validate_body(card, &bound).is_err());
    }
}
