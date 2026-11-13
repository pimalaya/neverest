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
//!
//! The `Meta` tier is each connector's own: one whose source serves the
//! header fields raw hands them to io-pimdir's `derive_meta`, the reading
//! a body gets, and this crate parses no field itself. A source serving a
//! field decoded alone names its member from that value, which stands
//! until the body is read (Annex A.1).

use io_pimdir::summary::{PimdirDerivation, mail};

/// The `Full`-tier derivation: the summary a listing names, read off a raw
/// message's headers.
///
/// `raw` is only the header prefix the stream captured, so the octet count is
/// restated from `size` and the attachment mark is left unknown: no part is
/// walked here. The store's write walks each body entering it, once, and
/// states the mark and the invitation over this summary, or takes them from a
/// row already holding the body (pimdir STORAGE Annex A.1, §14.3).
pub fn parse_body(raw: &[u8], size: u64) -> PimdirDerivation {
    let mut derivation = mail::derive(raw);

    if let Some(io_pimdir::summary::PimdirSummary::Mail(summary)) = &mut derivation.summary {
        summary.size = (size > 0).then_some(size);
        summary.attachment = None;
    }

    derivation
}

/// The pimdir summary vectors a connector's reading is held to (pimdir
/// vectors/summaries.json, STORAGE Annex A.1): the fixtures another parser
/// reads another way, copied under tests/vectors/fixtures with what the
/// vectors state for each.
#[cfg(all(test, any(feature = "imap", feature = "gmail", feature = "msgraph")))]
#[cfg_attr(
    not(all(feature = "imap", feature = "gmail", feature = "msgraph")),
    allow(dead_code)
)]
pub(crate) mod vectors {
    use io_pimdir::summary::{PimdirDerivation, PimdirSummary};

    /// The `date` every vector below states.
    const DATE: &str = "2026-08-01T10:00:00Z";

    /// One fixture and the columns the vectors state for it.
    pub(crate) struct Vector {
        /// The fixture's name under tests/vectors/fixtures.
        pub(crate) name: &'static str,
        /// The whole message.
        pub(crate) body: &'static [u8],
        pub(crate) link_id: &'static str,
        pub(crate) message_id: Option<&'static str>,
        pub(crate) in_reply_to: &'static [&'static str],
        pub(crate) subject: &'static str,
        pub(crate) sender: &'static str,
        pub(crate) sender_name: &'static str,
    }

    macro_rules! fixture {
        ($name:literal) => {
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/vectors/fixtures/",
                $name,
                ".eml"
            ))
        };
    }

    pub(crate) const VECTORS: [Vector; 10] = [
        Vector {
            name: "mail-id-comment",
            body: fixture!("mail-id-comment"),
            link_id: "comment-1@example.org",
            message_id: Some("comment-1@example.org"),
            in_reply_to: &["p-1@example.org", "p-2@example.org"],
            subject: "Commented ids",
            sender: "ops@example.org",
            sender_name: "Night Ops",
        },
        Vector {
            name: "mail-bracketless-id",
            body: fixture!("mail-bracketless-id"),
            link_id: "bare-2@example.org",
            message_id: Some("bare-2@example.org"),
            in_reply_to: &[],
            subject: "No brackets",
            sender: "alice@example.org",
            sender_name: "Alice Example",
        },
        Vector {
            name: "mail-repeated-fields",
            body: fixture!("mail-repeated-fields"),
            link_id: "repeated-1@example.org",
            message_id: Some("repeated-1@example.org"),
            in_reply_to: &[],
            subject: "First subject",
            sender: "first@example.org",
            sender_name: "First Sender",
        },
        Vector {
            name: "mail-folded-subject",
            body: fixture!("mail-folded-subject"),
            link_id: "alt:Café crème and\t  notes|2026-08-01T10:00:00Z|alice@example.org",
            message_id: None,
            in_reply_to: &[],
            subject: "Café crème and\t  notes",
            sender: "alice@example.org",
            sender_name: "Alice Example",
        },
        Vector {
            name: "mail-military-zone",
            body: fixture!("mail-military-zone"),
            link_id: "alt:Military zone|2026-08-01T10:00:00Z|alice@example.org",
            message_id: None,
            in_reply_to: &[],
            subject: "Military zone",
            sender: "alice@example.org",
            sender_name: "Alice Example",
        },
        Vector {
            name: "mail-charset-gbk",
            body: fixture!("mail-charset-gbk"),
            link_id: "charset-gbk@example.org",
            message_id: Some("charset-gbk@example.org"),
            in_reply_to: &[],
            subject: "你好世界",
            sender: "alice@example.org",
            sender_name: "Alice Example",
        },
        Vector {
            name: "mail-charset-iso-2022-jp",
            body: fixture!("mail-charset-iso-2022-jp"),
            link_id: "charset-jis@example.org",
            message_id: Some("charset-jis@example.org"),
            in_reply_to: &[],
            subject: "こんにちは",
            sender: "alice@example.org",
            sender_name: "Alice Example",
        },
        Vector {
            name: "mail-charset-iso-8859-2",
            body: fixture!("mail-charset-iso-8859-2"),
            link_id: "charset-latin2@example.org",
            message_id: Some("charset-latin2@example.org"),
            in_reply_to: &[],
            subject: "Zażółć gęślą jaźń",
            sender: "lukasz@example.org",
            sender_name: "Łukasz Żak",
        },
        Vector {
            name: "mail-charset-replacement",
            body: fixture!("mail-charset-replacement"),
            link_id: "charset-replacement@example.org",
            message_id: Some("charset-replacement@example.org"),
            in_reply_to: &[],
            subject: "=?ISO-2022-KR?B?GyQpQw5HUQ8=?= kept",
            sender: "alice@example.org",
            sender_name: "Alice Example",
        },
        Vector {
            name: "mail-charset-windows-1252",
            body: fixture!("mail-charset-windows-1252"),
            link_id: "alt:Coût 10 €|2026-08-01T10:00:00Z|alice@example.org",
            message_id: None,
            in_reply_to: &[],
            subject: "Coût 10 €",
            sender: "alice@example.org",
            sender_name: "Alice Example",
        },
    ];

    impl Vector {
        /// The header block, its closing empty line included.
        pub(crate) fn header(&self) -> &'static [u8] {
            let end = self.body.windows(4).position(|at| at == b"\r\n\r\n");
            &self.body[..end.expect("a fixture holds a header block") + 4]
        }

        /// Each field's name and value in document order, as a source
        /// listing header fields in JSON serves them: the value trimmed
        /// at both ends, the line breaks of a folded one kept.
        pub(crate) fn fields(&self) -> Vec<(String, String)> {
            let header = str::from_utf8(self.header()).expect("an ASCII header block");
            let mut fields: Vec<(String, String)> = Vec::new();
            for line in header.split_inclusive("\r\n") {
                match fields.last_mut() {
                    Some((_, value)) if line.starts_with([' ', '\t']) => value.push_str(line),
                    _ => {
                        if let Some((name, value)) = line.split_once(':') {
                            fields.push((name.to_owned(), value.to_owned()));
                        }
                    }
                }
            }
            for (_, value) in &mut fields {
                *value = value.trim().to_owned();
            }
            fields
        }

        /// Asserts `derivation` names the fixture as the vectors state.
        pub(crate) fn assert_named(&self, derivation: &PimdirDerivation) {
            let name = self.name;
            let Some(PimdirSummary::Mail(summary)) = &derivation.summary else {
                panic!("{name}: expected a mail summary, got {derivation:?}");
            };

            assert_eq!(derivation.link_id.as_str(), self.link_id, "{name}");
            assert_eq!(summary.message_id.as_deref(), self.message_id, "{name}");
            assert_eq!(summary.in_reply_to, self.in_reply_to, "{name}");
            assert_eq!(summary.subject, self.subject, "{name}");
            assert_eq!(summary.sender.as_deref(), Some(self.sender), "{name}");
            assert_eq!(
                summary.sender_name.as_deref(),
                Some(self.sender_name),
                "{name}"
            );
            assert_eq!(summary.date.as_deref(), Some(DATE), "{name}");
            assert_eq!(derivation.sort_key.as_str(), DATE, "{name}");
            assert_eq!(summary.attachment, Some(false), "{name}");
        }
    }
}

#[cfg(test)]
mod tests {
    use io_pimdir::summary::{PimdirSummary, mail::PimdirMailSummary};

    use super::*;

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
}
