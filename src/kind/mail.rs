//! The `message/rfc822` kind: how a mail message derives its link id and
//! its `v:1` summary.
//!
//! The **conventions** are the format's: the link id is the bare
//! `Message-ID` and the `Date:` is the UTC instant, both as pimdir SPEC
//! Annex A.1 and the format's own `vectors/meta.json` give them, and the
//! summary is [`PimdirMailMeta`] itself, so the schema cannot drift from
//! io-pimdir's by a field or a spelling.
//!
//! The **reader** is still this crate's, and deliberately.
//! [`io_pimdir::conventions::mail`] scans headers raw, so it hands back
//! `=?utf-8?q?D=C3=A9p=C3=B4t?=` where a subject was RFC 2047 encoded, and a
//! list of a real mailbox is then mojibake. The format's vectors are ASCII
//! only, so nothing there says otherwise and nothing there catches it.
//! Delegating the scan would trade a correct reader for a nominal
//! deduplication; `a_subject_is_decoded_not_shown_encoded` holds the line
//! until io-pimdir decodes encoded-words, at which point `parse_body` becomes
//! a call to `mail::derive` and this file loses its parser.
//!
//! Mail is also the one kind with a cheap **`Meta`** tier, an IMAP or Graph
//! `ENVELOPE` rather than a body, which no library derives for us anyway.
//!
//! So there are **two** derivations and they must agree byte-for-byte. They
//! did not once: `chrono`'s plain `to_rfc3339` writes UTC as `+00:00` while
//! `mail_parser` writes `Z`, and since the `alt:` link id embeds the date, a
//! message with no `Message-ID` linked one way at `Meta` and another at
//! `Full`. That stranded the `Meta` item, so it was re-fetched every single
//! sync and its body was stored twice. Both tiers now build the *same*
//! [`PimdirMailMeta`] and take their link id and sort key from it through
//! [`link_id`] and [`sort_key`], over one date formatter ([`utc`]), so there
//! is one rule and one spelling; `meta_and_full_link_ids_agree_on_dates`
//! keeps them honest.
//!
//! The DAV kinds have no such hazard: they resolve at `Full` only, so there
//! is only ever one derivation to get right.

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use io_pimdir::conventions::mail::PimdirMailMeta;
use io_replica::placement::{ReplicaLinkId, ReplicaMeta, ReplicaSortKey};
use mail_parser::{HeaderValue, MessageParser};

use crate::item::summary::{ItemSummary, normalize_message_id, parse_message_ids};

/// The link id for a message: its bare `Message-ID`, else a
/// `(subject, date, sender)` fallback (`alt:`).
///
/// pimdir SPEC Annex A.1 and the format's vectors give the identity as the
/// bare id, angle brackets stripped and nothing prepended: a `Message-ID`
/// cannot contain a colon before its `@` (RFC 5322 `atext`), so the
/// prefixed fallbacks can never be mistaken for one.
///
/// Both tiers go through this, over the meta they both build, so the rule
/// is written once.
fn link_id(meta: &PimdirMailMeta) -> ReplicaLinkId {
    match &meta.message_id {
        Some(id) if !id.is_empty() => ReplicaLinkId::from(id.clone()),
        _ => ReplicaLinkId::from(format!(
            "alt:{}|{}|{}",
            meta.subject,
            meta.date.as_deref().unwrap_or_default(),
            meta.from.as_deref().unwrap_or_default(),
        )),
    }
}

/// The message's position in a newest-first listing: the `Date:` the meta
/// already carries, which is RFC 3339 UTC at seconds precision (pimdir SPEC
/// Annex A.1), so byte order is chronological order whatever offset the
/// sender wrote.
///
/// Unknown (empty) when the header is missing or unparseable, which lands
/// the message at the end of the listing.
fn sort_key(meta: &PimdirMailMeta) -> ReplicaSortKey {
    ReplicaSortKey(meta.date.clone().unwrap_or_default())
}

/// The `Date:` as Annex A.1 spells it: the **UTC instant**, RFC 3339 at
/// seconds precision, never the local reading the sender wrote.
///
/// The one formatter both tiers use, which is what keeps the `alt:` link id
/// they each derive identical.
fn utc(date: DateTime<FixedOffset>) -> String {
    date.with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// The `Meta`-tier derivation: link id, summary and sort key from an
/// IMAP/Graph envelope, with no body fetched.
pub fn parse_summary(env: &ItemSummary) -> (ReplicaLinkId, ReplicaMeta, ReplicaSortKey) {
    let meta = PimdirMailMeta {
        v: 1,
        message_id: env.message_id.clone().filter(|id| !id.is_empty()),
        in_reply_to: env.in_reply_to.clone(),
        subject: env.subject.clone(),
        from: env.from.first().map(|a| a.email.clone()),
        to: env.to.first().map(|a| a.email.clone()),
        date: env.date.map(utc),
        size: (env.size > 0).then_some(env.size),
    };

    finish(meta)
}

/// The `Full`-tier derivation: the same summary, read off a raw message's
/// headers. `size` is the whole message's octet length, known from the
/// stream, since `raw` is only the header prefix it carried.
pub fn parse_body(raw: &[u8], size: u64) -> (ReplicaLinkId, ReplicaMeta, ReplicaSortKey) {
    let parsed = MessageParser::default().parse(raw);
    let address = |header: Option<&mail_parser::Address<'_>>| {
        header
            .and_then(|addrs| addrs.first())
            .and_then(|a| a.address())
            .map(str::to_string)
    };

    let meta = PimdirMailMeta {
        v: 1,
        message_id: parsed
            .as_ref()
            .and_then(|m| m.message_id())
            .and_then(normalize_message_id),
        in_reply_to: parsed
            .as_ref()
            .map(|m| match m.in_reply_to() {
                HeaderValue::TextList(ids) => ids
                    .iter()
                    .filter_map(|id| normalize_message_id(id))
                    .collect(),
                HeaderValue::Text(id) => parse_message_ids(id),
                _ => Vec::new(),
            })
            .unwrap_or_default(),
        subject: parsed
            .as_ref()
            .and_then(|m| m.subject())
            .unwrap_or_default()
            .to_string(),
        from: address(parsed.as_ref().and_then(|m| m.from())),
        to: address(parsed.as_ref().and_then(|m| m.to())),
        date: parsed
            .as_ref()
            .and_then(|m| m.date())
            .and_then(|d| DateTime::parse_from_rfc3339(&d.to_rfc3339()).ok())
            .map(utc),
        size: (size > 0).then_some(size),
    };

    finish(meta)
}

/// The three values a tier reports, from the meta it built.
fn finish(meta: PimdirMailMeta) -> (ReplicaLinkId, ReplicaMeta, ReplicaSortKey) {
    let link = link_id(&meta);
    let key = sort_key(&meta);
    let meta = ReplicaMeta(serde_json::to_string(&meta).unwrap_or_default());

    (link, meta, key)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::item::address::Address;

    /// A reader's view of the `message/rfc822` meta (pimdir SPEC Annex A), mirroring
    /// what a Himalaya pimdir backend would deserialize.
    #[derive(Deserialize)]
    struct MetaView {
        v: u8,
        message_id: Option<String>,
        subject: String,
        from: Option<String>,
        to: Option<String>,
        date: Option<String>,
        size: Option<u64>,
    }

    fn addr(email: &str) -> Address {
        Address {
            name: None,
            email: email.to_string(),
        }
    }

    fn summary(message_id: Option<&str>, date: Option<&str>) -> ItemSummary {
        ItemSummary {
            id: "1".into(),
            message_id: message_id.map(str::to_string),
            in_reply_to: Vec::new(),
            flags: Default::default(),
            subject: "Stand-up notes".into(),
            from: vec![addr("alice@example.org")],
            to: vec![addr("bob@example.org")],
            date: date.map(|d| DateTime::parse_from_rfc3339(d).unwrap()),
            size: 299,
            has_attachment: None,
        }
    }

    /// The body tier is io-pimdir's, so a message the format has a vector for
    /// derives what the vector says: the identity is the bare `Message-ID`,
    /// and the date is the UTC instant rather than the offset the sender
    /// wrote.
    #[test]
    fn the_body_tier_is_the_format() {
        let raw = b"Message-ID: <basic-1@example.org>\r\n\
                    Subject: Stand-up notes\r\n\
                    From: alice@example.org\r\n\
                    To: bob@example.org\r\n\
                    Date: Sat, 1 Aug 2026 12:00:00 +0200\r\n\r\nbody";

        let (link, meta, key) = parse_body(raw, 299);
        let view: MetaView = serde_json::from_str(&meta.0).unwrap();

        assert_eq!(link.0, "basic-1@example.org");
        assert_eq!(view.v, 1);
        assert_eq!(view.message_id.as_deref(), Some("basic-1@example.org"));
        assert_eq!(view.date.as_deref(), Some("2026-08-01T10:00:00Z"));
        assert_eq!(key.0, "2026-08-01T10:00:00Z");
        assert_eq!(view.subject, "Stand-up notes");
        assert_eq!(view.from.as_deref(), Some("alice@example.org"));
        assert_eq!(view.to.as_deref(), Some("bob@example.org"));
    }

    /// A summary is what a reader shows, so an encoded-word header is decoded
    /// here or it is mojibake in every list. This is the one thing keeping
    /// `parse_body` from being a call to io-pimdir's conventions, whose
    /// scanner reads headers raw and whose vectors are ASCII only, so nothing
    /// upstream says otherwise and nothing upstream catches it.
    #[test]
    fn a_subject_is_decoded_not_shown_encoded() {
        let raw = b"Message-ID: <enc-1@example.org>\r\n\
                    Subject: =?utf-8?q?D=C3=A9p=C3=B4t_de_votre_Lettre?=\r\n\
                    From: alice@example.org\r\n\r\nbody";

        let (_, meta, _) = parse_body(raw, 299);
        let view: MetaView = serde_json::from_str(&meta.0).unwrap();

        assert_eq!(view.subject, "Dépôt de votre Lettre");
    }

    /// The stream knows the message's length; the header prefix the
    /// conventions read does not, and reporting the prefix would show every
    /// message as a few hundred bytes.
    #[test]
    fn the_body_tier_reports_the_streamed_size_not_the_prefix() {
        let raw = b"Message-ID: <x@y>\r\nSubject: S\r\n\r\n";
        let (_, meta, _) = parse_body(raw, 168_320);
        let view: MetaView = serde_json::from_str(&meta.0).unwrap();

        assert_eq!(view.size, Some(168_320));
    }

    /// The two tiers derive one message one way. The `alt:` link id embeds
    /// the date, so a date the tiers spell differently splits the message in
    /// two: re-fetched every sync, its body stored twice.
    #[test]
    fn meta_and_full_link_ids_agree_on_dates() {
        let raw = b"Subject: Stand-up notes\r\n\
                    From: alice@example.org\r\n\
                    To: bob@example.org\r\n\
                    Date: Sat, 1 Aug 2026 12:00:00 +0200\r\n\r\nbody";

        let (body_link, body_meta, body_key) = parse_body(raw, 299);
        let (env_link, env_meta, env_key) =
            parse_summary(&summary(None, Some("2026-08-01T12:00:00+02:00")));

        assert!(body_link.0.starts_with("alt:"), "got {}", body_link.0);
        assert_eq!(body_link, env_link);
        assert_eq!(body_key, env_key);
        assert_eq!(body_meta.0, env_meta.0);
    }

    /// The same message, whichever tier resolved it, is one item.
    #[test]
    fn both_tiers_link_a_message_by_its_bare_id() {
        let raw = b"Message-ID: <basic-1@example.org>\r\n\
                    Subject: Stand-up notes\r\n\
                    From: alice@example.org\r\n\
                    To: bob@example.org\r\n\
                    Date: Sat, 1 Aug 2026 12:00:00 +0200\r\n\r\nbody";

        let (body_link, ..) = parse_body(raw, 299);
        let (env_link, ..) = parse_summary(&summary(
            Some("basic-1@example.org"),
            Some("2026-08-01T12:00:00+02:00"),
        ));

        assert_eq!(body_link.0, "basic-1@example.org");
        assert_eq!(body_link, env_link);
    }

    /// A message with no parseable date omits it rather than guessing, and
    /// the empty key lands it last in a descending listing.
    #[test]
    fn a_message_without_a_date_sorts_last_and_omits_the_field() {
        let (_, meta, key) = parse_summary(&summary(Some("nodate-1@example.org"), None));
        let view: MetaView = serde_json::from_str(&meta.0).unwrap();

        assert_eq!(view.date, None);
        assert!(key.0.is_empty());
    }
}
