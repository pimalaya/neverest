//! The `message/rfc822` kind: how a mail message derives its link id and
//! its `v:1` summary.
//!
//! Mail is the one kind with **two** derivations — a cheap `Meta` tier from
//! the IMAP `ENVELOPE` ([`parse_summary`]) and the `Full` tier from the
//! parsed body ([`parse_body`]) — so the two must agree byte-for-byte. They
//! did not once: `chrono`'s plain `to_rfc3339` writes UTC as `+00:00` while
//! `mail_parser` writes `Z`, and since the `alt:` link id embeds the date, a
//! message with no `Message-ID` linked one way at `Meta` and another at
//! `Full`. That stranded the `Meta` item, so it was re-fetched every single
//! sync and its body was stored twice. [`envelope_date`] is the one
//! canonical formatting both paths go through, and
//! `meta_and_full_link_ids_agree_on_dates` keeps them honest.
//!
//! The DAV kinds will have no such hazard: they resolve at `Full` only, so
//! there is only ever one derivation to get right.

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use io_replica::placement::{ReplicaLinkId, ReplicaMeta, ReplicaSortKey};
use mail_parser::{HeaderValue, MessageParser};
use serde::Serialize;

use crate::item::summary::{ItemSummary, normalize_message_id, parse_message_ids};

/// The `message/rfc822` meta schema version this writer emits (pimdir SPEC Annex A).
const META_VERSION: u8 = 1;

/// Versioned mail-envelope summary persisted as the `message/rfc822`
/// [`ReplicaMeta`] blob — the stable JSON contract a reader (e.g. a Himalaya
/// pimdir backend) parses to render an envelope list without fetching a body.
/// Documented in the pimdir SPEC (Annex A, "Application meta conventions"); absent
/// optional fields mean "unknown". Flags do not live here (they are
/// `items.flags`).
#[derive(Serialize)]
struct MetaSummary<'a> {
    /// Schema version ([`META_VERSION`]).
    v: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_id: Option<&'a str>,
    /// The `In-Reply-To:` ids, bare and normalised like `message_id`
    /// (pimdir SPEC Annex A.1), so a reply pairs with its parent from a
    /// listing rather than from a body.
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    in_reply_to: &'a [String],
    subject: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
}

/// The link id for an enumerated/fetched message: normalized `Message-ID`
/// (`mid:`), else a `(subject, date, sender)` fallback (`alt:`).
fn link_id(
    message_id: Option<&str>,
    subject: &str,
    date: Option<&str>,
    from: Option<&str>,
) -> ReplicaLinkId {
    match message_id {
        Some(mid) if !mid.trim().is_empty() => {
            ReplicaLinkId::from(format!("mid:{}", mid.trim().trim_matches(['<', '>'])))
        }
        _ => {
            let date = date.unwrap_or("");
            let from = from.unwrap_or("");
            ReplicaLinkId::from(format!("alt:{subject}|{date}|{from}"))
        }
    }
}

/// Formats a date for the link id and meta the **same way `mail_parser` does**
/// in the `Full` path (`DateTime::to_rfc3339`): UTC as `Z`, an offset as
/// `+hh:mm`, seconds precision. See the module header for why this matters.
fn envelope_date(date: DateTime<FixedOffset>) -> String {
    date.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// The message's position in a newest-first listing: its `Date:` header in
/// RFC 3339 **UTC** at seconds precision (pimdir SPEC Annex A.1), so byte
/// order is chronological order whatever offset the sender wrote.
///
/// Both derivations go through it, over the same RFC 3339 string they put
/// in the meta, so a message keeps its place when its body is hydrated.
/// Unknown (empty) when the header is missing or unparseable, which lands
/// the message at the end of the listing.
fn sort_key(date: Option<&str>) -> ReplicaSortKey {
    let Some(date) = date.and_then(|date| DateTime::parse_from_rfc3339(date).ok()) else {
        return ReplicaSortKey::default();
    };

    let date = date.with_timezone(&Utc);

    ReplicaSortKey(date.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// The `Meta`-tier derivation: link id, summary and sort key from an
/// IMAP/Graph envelope, with no body fetched.
pub fn parse_summary(env: &ItemSummary) -> (ReplicaLinkId, ReplicaMeta, ReplicaSortKey) {
    let from = env.from.first().map(|a| a.email.clone());
    let to = env.to.first().map(|a| a.email.clone());
    let date = env.date.map(envelope_date);

    let link = link_id(
        env.message_id.as_deref(),
        &env.subject,
        date.as_deref(),
        from.as_deref(),
    );
    let key = sort_key(date.as_deref());
    let summary = MetaSummary {
        v: META_VERSION,
        message_id: env.message_id.as_deref(),
        in_reply_to: &env.in_reply_to,
        subject: &env.subject,
        from,
        to,
        date,
        size: (env.size > 0).then_some(env.size),
    };
    let meta = ReplicaMeta(serde_json::to_string(&summary).unwrap_or_default());
    (link, meta, key)
}

/// The `Full`-tier derivation: link id, summary and sort key from a raw
/// message's headers. `size` is the full message's octet length (known from
/// the stream), carried into the meta so a reader shows it without the body.
pub fn parse_body(raw: &[u8], size: u64) -> (ReplicaLinkId, ReplicaMeta, ReplicaSortKey) {
    let parsed = MessageParser::default().parse(raw);
    let message_id = parsed.as_ref().and_then(|m| m.message_id());
    let in_reply_to: Vec<String> = parsed
        .as_ref()
        .map(|m| match m.in_reply_to() {
            HeaderValue::TextList(ids) => ids
                .iter()
                .filter_map(|id| normalize_message_id(id))
                .collect(),
            HeaderValue::Text(id) => parse_message_ids(id),
            _ => Vec::new(),
        })
        .unwrap_or_default();
    let subject = parsed.as_ref().and_then(|m| m.subject()).unwrap_or("");
    let from = parsed
        .as_ref()
        .and_then(|m| m.from())
        .and_then(|addrs| addrs.first())
        .and_then(|a| a.address())
        .map(|s| s.to_string());
    let to = parsed
        .as_ref()
        .and_then(|m| m.to())
        .and_then(|addrs| addrs.first())
        .and_then(|a| a.address())
        .map(|s| s.to_string());
    let date = parsed
        .as_ref()
        .and_then(|m| m.date())
        .map(|d| d.to_rfc3339());

    let link = link_id(message_id, subject, date.as_deref(), from.as_deref());
    let key = sort_key(date.as_deref());
    let summary = MetaSummary {
        v: META_VERSION,
        message_id,
        in_reply_to: &in_reply_to,
        subject,
        from,
        to,
        date,
        size: (size > 0).then_some(size),
    };
    let meta = ReplicaMeta(serde_json::to_string(&summary).unwrap_or_default());
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
            email: email.into(),
        }
    }

    #[test]
    fn meta_and_full_link_ids_agree_on_dates() {
        // NOTE: the `alt:` link id embeds the date, so the Meta and the Full
        // path must format it identically. Otherwise the same message links
        // differently at each tier and is re-fetched on every sync.
        for date_hdr in [
            "Tue, 16 Apr 2019 11:40:14 +0000",
            "Tue, 16 May 2023 13:06:22 +0200",
        ] {
            let raw = format!(
                "From: Alice <alice@example.org>\r\n\
                 Subject: Hello there\r\n\
                 Date: {date_hdr}\r\n\r\nbody"
            );
            let (full_link, _, full_key) = parse_body(raw.as_bytes(), 10);

            let env = ItemSummary {
                id: "1".into(),
                in_reply_to: Vec::new(),
                message_id: None,
                flags: Default::default(),
                subject: "Hello there".into(),
                from: vec![addr("alice@example.org")],
                to: vec![],
                date: Some(chrono::DateTime::parse_from_rfc2822(date_hdr).unwrap()),
                size: 10,
                has_attachment: None,
            };
            let (meta_link, _, meta_key) = parse_summary(&env);

            assert!(
                full_link.0.starts_with("alt:"),
                "no Message-ID should yield an alt: link, got `{}`",
                full_link.0
            );
            assert_eq!(
                meta_link.0, full_link.0,
                "Meta and Full link ids must match for date `{date_hdr}`"
            );
            assert_eq!(
                meta_key, full_key,
                "Meta and Full sort keys must match for date `{date_hdr}`"
            );
        }
    }

    #[test]
    fn the_sort_key_is_the_date_in_utc_at_a_fixed_width() {
        // NOTE: byte order is the order (pimdir SPEC §9.3), so a zoned date has
        // to land in UTC: `+02:00` and `Z` sort apart while naming one instant.
        let raw = b"Subject: Zoned\r\nDate: Tue, 16 May 2023 13:06:22 +0200\r\n\r\nbody";
        let (_, _, key) = parse_body(raw, 10);
        assert_eq!(key.0, "2023-05-16T11:06:22Z");

        let (_, _, key) = parse_body(b"Subject: Undated\r\n\r\nbody", 10);
        assert!(key.is_unknown(), "an undated message sorts as unknown");
    }

    #[test]
    fn parse_summary_emits_the_v1_schema() {
        let env = ItemSummary {
            id: "42".into(),
            in_reply_to: Vec::new(),
            message_id: Some("abc@host".into()),
            flags: Default::default(),
            subject: "Hello".into(),
            from: vec![addr("alice@example.org")],
            to: vec![addr("bob@example.org")],
            date: None,
            size: 1234,
            has_attachment: None,
        };

        let (_, meta, _) = parse_summary(&env);
        let view: MetaView = serde_json::from_str(&meta.0).unwrap();
        assert_eq!(view.v, 1);
        assert_eq!(view.message_id.as_deref(), Some("abc@host"));
        assert_eq!(view.subject, "Hello");
        assert_eq!(view.from.as_deref(), Some("alice@example.org"));
        assert_eq!(view.to.as_deref(), Some("bob@example.org"));
        assert_eq!(view.date, None);
        assert_eq!(view.size, Some(1234));
    }

    #[test]
    fn parse_body_emits_the_v1_schema_with_size() {
        let raw = b"Message-ID: <mid@host>\r\n\
                    From: Alice <alice@example.org>\r\n\
                    To: Bob <bob@example.org>\r\n\
                    Subject: Streamed\r\n\r\nbody";
        let (link, meta, _) = parse_body(raw, 4096);
        assert_eq!(link.0, "mid:mid@host");

        let view: MetaView = serde_json::from_str(&meta.0).unwrap();
        assert_eq!(view.v, 1);
        assert_eq!(view.subject, "Streamed");
        assert_eq!(view.from.as_deref(), Some("alice@example.org"));
        assert_eq!(view.to.as_deref(), Some("bob@example.org"));
        assert_eq!(view.size, Some(4096));
    }

    #[test]
    fn absent_optionals_are_omitted_not_null() {
        let (_, meta, _) = parse_body(b"Subject: Bare\r\n\r\n", 0);
        assert!(
            !meta.0.contains("null"),
            "absent fields are omitted: {}",
            meta.0
        );
        let view: MetaView = serde_json::from_str(&meta.0).unwrap();
        assert_eq!(view.subject, "Bare");
        assert_eq!(view.from, None);
        assert_eq!(view.size, None);
    }
}
