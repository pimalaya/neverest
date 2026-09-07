//! # Mail kind
//!
//! `message/rfc822`: how a message derives its link id, its summary and its
//! sort key. The derivations are io-pimdir's (pimdir STORAGE Annex A.1), so
//! the schema cannot drift from the format's by a field or a spelling.
//!
//! Mail is the one kind with a cheap `Meta` tier, so its two derivations
//! must agree byte-for-byte. The `alt:` link id embeds the date: spell it
//! differently across tiers and a message with no `Message-ID` splits in
//! two, re-fetched every sync and stored twice.

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use io_pimdir::summary::{
    PimdirAddress, PimdirDerivation,
    mail::{self, PimdirMailSummary},
};

use crate::item::{address::Address, summary::ItemSummary};

/// The `Meta`-tier derivation: link id, summary and sort key from an
/// IMAP/Graph envelope, with no body fetched.
///
/// The envelope walks no MIME part, so `attachment` stays unknown unless the
/// backend read it off the body structure.
pub fn parse_summary(env: &ItemSummary) -> PimdirDerivation {
    let from: Vec<PimdirAddress> = env.from.iter().map(address).collect();
    let first = from.first();

    PimdirMailSummary {
        message_id: env.message_id.clone().filter(|id| !id.is_empty()),
        in_reply_to: env.in_reply_to.clone(),
        subject: env.subject.clone(),
        sender: first.map(|address| address.address.clone()),
        sender_name: first.and_then(|address| address.name.clone()),
        date: env.date.map(utc),
        size: (env.size > 0).then_some(env.size),
        attachment: env.has_attachment,
        from,
        to: env.to.iter().map(address).collect(),
        cc: env.cc.iter().map(address).collect(),
        bcc: env.bcc.iter().map(address).collect(),
    }
    .derivation()
}

/// The `Full`-tier derivation: the same summary, read off a raw message's
/// headers.
///
/// `raw` is only the header prefix the stream captured, so the octet count is
/// restated from `size` and no part was walked for an attachment.
pub fn parse_body(raw: &[u8], size: u64) -> PimdirDerivation {
    let mut derivation = mail::derive(raw);

    if let Some(io_pimdir::summary::PimdirSummary::Mail(summary)) = &mut derivation.summary {
        summary.size = (size > 0).then_some(size);
        summary.attachment = None;
    }

    derivation
}

/// The `Date:` as Annex A.1 spells it: the UTC instant, RFC 3339 at seconds
/// precision, never the local reading the sender wrote.
fn utc(date: DateTime<FixedOffset>) -> String {
    date.with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// An envelope address as Annex A.6 records it: the addr-spec canonical, the
/// display name as the backend decoded it.
fn address(address: &Address) -> PimdirAddress {
    PimdirAddress {
        address: PimdirAddress::canonical(&address.email),
        name: address.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use io_pimdir::summary::PimdirSummary;

    use super::*;

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
            from: vec![addr("Alice@example.org")],
            to: vec![addr("bob@example.org")],
            cc: Vec::new(),
            bcc: Vec::new(),
            date: date.map(|d| DateTime::parse_from_rfc3339(d).unwrap()),
            size: 299,
            has_attachment: None,
        }
    }

    fn mail(derivation: &PimdirDerivation) -> &PimdirMailSummary {
        match derivation.summary.as_ref() {
            Some(PimdirSummary::Mail(summary)) => summary,
            other => panic!("expected a mail summary, got {other:?}"),
        }
    }

    /// The stream knows the message's length; the header prefix does not, and
    /// reporting the prefix would show every message as a few hundred bytes.
    #[test]
    fn the_body_tier_reports_the_streamed_size_not_the_prefix() {
        let raw = b"Message-ID: <x@y>\r\nSubject: S\r\n\r\n";
        let derivation = parse_body(raw, 168_320);

        assert_eq!(derivation.link_id.as_str(), "x@y");
        assert_eq!(mail(&derivation).size, Some(168_320));
        assert_eq!(mail(&derivation).attachment, None, "no part was walked");
    }

    /// The two tiers derive one message one way. The `alt:` link id embeds
    /// the date, so a date the tiers spell differently splits the message in
    /// two: re-fetched every sync, its body stored twice.
    #[test]
    fn meta_and_full_derivations_agree() {
        let raw = b"Subject: Stand-up notes\r\n\
                    From: Alice@example.org\r\n\
                    To: bob@example.org\r\n\
                    Date: Sat, 1 Aug 2026 12:00:00 +0200\r\n\r\nbody";

        let body = parse_body(raw, 299);
        let envelope = parse_summary(&summary(None, Some("2026-08-01T12:00:00+02:00")));

        assert!(
            body.link_id.as_str().starts_with("alt:"),
            "{:?}",
            body.link_id
        );
        assert_eq!(body.link_id, envelope.link_id);
        assert_eq!(body.sort_key.as_str(), "2026-08-01T10:00:00Z");
        assert_eq!(body.sort_key, envelope.sort_key);
        assert_eq!(mail(&body), mail(&envelope));
    }

    /// The same message, whichever tier resolved it, is one item.
    #[test]
    fn both_tiers_link_a_message_by_its_bare_id() {
        let raw = b"Message-ID: <basic-1@example.org>\r\n\
                    Subject: Stand-up notes\r\n\
                    From: alice@example.org\r\n\
                    To: bob@example.org\r\n\
                    Date: Sat, 1 Aug 2026 12:00:00 +0200\r\n\r\nbody";

        let body = parse_body(raw, 299);
        let envelope = parse_summary(&summary(
            Some("basic-1@example.org"),
            Some("2026-08-01T12:00:00+02:00"),
        ));

        assert_eq!(body.link_id.as_str(), "basic-1@example.org");
        assert_eq!(body.link_id, envelope.link_id);
    }

    /// A message with no parseable date omits it rather than guessing, the
    /// empty key landing it last in a descending listing.
    #[test]
    fn a_message_without_a_date_sorts_last_and_omits_the_field() {
        let derivation = parse_summary(&summary(Some("nodate-1@example.org"), None));

        assert_eq!(mail(&derivation).date, None);
        assert!(derivation.sort_key.is_unknown());
    }
}
