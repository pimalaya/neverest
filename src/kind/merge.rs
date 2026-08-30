//! # Three-way merge
//!
//! What a run resolves a content conflict with. Most conflicts are not
//! disagreements: one side changed a phone number and the other a note, and
//! the base proves it by naming which side touched which field.
//!
//! Built in rather than configured, at build time too: it rides on the `dav`
//! cargo feature rather than one of its own, every mutable-content kind
//! arriving with `dav`. Contacts are vcard-rs, calendars and tasks and
//! journals ical-rs, and mail is immutable-content and reaches none of this.
//!
//! Because it cannot be swapped it is strictly conservative, resolving on an
//! empty report and on nothing else: a merge nobody can replace has no
//! business deciding what a person might have decided differently. The local
//! body is the left side, so the store's own bytes survive byte for byte.

#[cfg(feature = "dav")]
use ical::tree::{cst::IcalCst, merge::IcalMerge};
#[cfg(feature = "dav")]
use vcard::tree::{cst::VcardCst, merge::VcardMerge};

use crate::kind::Kind;

/// What a three-way merge concluded about one conflicted item.
// NOTE: a build without `dav` carries no mutable-content kind, so mail is
// the only arm left and neither resolving variant is ever constructed.
#[cfg_attr(not(feature = "dav"), allow(dead_code))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Merged {
    /// Nobody disagreed: the merged body carries both sides' edits and
    /// resolves the conflict as an ordinary edit.
    Body(Vec<u8>),
    /// Both sides changed the same field, that many times over. No merge
    /// settles it, so the conflict parks for a person.
    Collided(usize),
    /// The merge could not run at all, for the reason named: a body no parser
    /// accepts, or a kind carrying no merge of its own. The conflict parks
    /// untouched.
    Unmergeable(String),
}

impl Kind {
    /// Three-way merges the `local` and `remote` bodies of one conflicted
    /// item against the `base` the last sync agreed on.
    ///
    /// The ical merge names no attendee the right side speaks for (RFC 5546
    /// §3.2): neverest syncs a calendar rather than acting as one. Preferring
    /// left decides nothing here, a run merging only on an empty report.
    // NOTE: without `dav` the mail arm is the whole match, and mail is
    // immutable-content, so no side is ever read.
    #[cfg_attr(not(feature = "dav"), allow(unused_variables))]
    pub fn merge(self, base: &[u8], local: &[u8], remote: &[u8]) -> Merged {
        match self {
            Self::Mail => Merged::Unmergeable(String::from("mail bodies are immutable")),
            #[cfg(feature = "dav")]
            Self::Vcard => {
                let (base, local, remote) = match (
                    VcardCst::parse(base),
                    VcardCst::parse(local),
                    VcardCst::parse(remote),
                ) {
                    (Ok(base), Ok(local), Ok(remote)) => (base, local, remote),
                    (base, local, remote) => {
                        return unparsed(base.err(), local.err(), remote.err());
                    }
                };

                let report = VcardMerge {
                    base: &base,
                    left: &local,
                    right: &remote,
                }
                .merge();

                match report.conflicts.len() {
                    0 => Merged::Body(report.merged.to_string().into_bytes()),
                    collided => Merged::Collided(collided),
                }
            }
            #[cfg(feature = "dav")]
            Self::Ical => {
                let (base, local, remote) = match (
                    IcalCst::parse(base),
                    IcalCst::parse(local),
                    IcalCst::parse(remote),
                ) {
                    (Ok(base), Ok(local), Ok(remote)) => (base, local, remote),
                    (base, local, remote) => {
                        return unparsed(base.err(), local.err(), remote.err());
                    }
                };

                let report = IcalMerge {
                    base: &base,
                    left: &local,
                    right: &remote,
                }
                .merge();

                match report.conflicts.len() {
                    0 => Merged::Body(report.merged.to_string().into_bytes()),
                    collided => Merged::Collided(collided),
                }
            }
        }
    }
}

/// Names the side whose body no parser accepts, for the log line a parked
/// conflict leaves behind, rather than counting it as a collision.
#[cfg(feature = "dav")]
fn unparsed<E: core::fmt::Display>(base: Option<E>, local: Option<E>, remote: Option<E>) -> Merged {
    let (side, err) = match (base, local, remote) {
        (Some(err), _, _) => ("base", err),
        (_, Some(err), _) => ("local", err),
        (_, _, Some(err)) => ("remote", err),
        _ => unreachable!("at least one side failed to parse"),
    };

    Merged::Unmergeable(format!("the {side} body does not parse: {err}"))
}

#[cfg(test)]
mod tests {
    use crate::kind::{Kind, merge::Merged};

    /// Disjoint edits are not a disagreement: the base names which side
    /// touched which field, so both survive.
    #[cfg(feature = "dav")]
    #[test]
    fn disjoint_edits_on_both_sides_merge_into_one_card() {
        let base =
            b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:+1\r\nNOTE:old\r\nEND:VCARD\r\n";
        let local =
            b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:+2\r\nNOTE:old\r\nEND:VCARD\r\n";
        let remote =
            b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:+1\r\nNOTE:new\r\nEND:VCARD\r\n";

        let Merged::Body(body) = Kind::Vcard.merge(base, local, remote) else {
            panic!("disjoint edits collide");
        };

        let body = String::from_utf8(body).unwrap();
        assert!(body.contains("TEL:+2"), "{body}");
        assert!(body.contains("NOTE:new"), "{body}");
    }

    /// The same field set two ways is the residual case no merge settles.
    #[cfg(feature = "dav")]
    #[test]
    fn a_same_field_collision_is_not_merged_away() {
        let base = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:+1\r\nEND:VCARD\r\n";
        let local = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:+2\r\nEND:VCARD\r\n";
        let remote = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:+3\r\nEND:VCARD\r\n";

        assert_eq!(Kind::Vcard.merge(base, local, remote), Merged::Collided(1));
    }

    /// The calendar half of the same rule, over the other library.
    #[cfg(feature = "dav")]
    #[test]
    fn a_calendar_merges_disjoint_edits_and_parks_a_collision() {
        let event = |summary: &str, location: &str| {
            format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//x//EN\r\nBEGIN:VEVENT\r\n\
                 UID:e1\r\nDTSTAMP:20260828T000000Z\r\nSUMMARY:{summary}\r\n\
                 LOCATION:{location}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
            )
            .into_bytes()
        };

        let base = event("Standup", "Room A");
        let local = event("Standup", "Room B");
        let remote = event("Daily", "Room A");

        let Merged::Body(body) = Kind::Ical.merge(&base, &local, &remote) else {
            panic!("disjoint edits collide");
        };
        let body = String::from_utf8(body).unwrap();
        assert!(body.contains("SUMMARY:Daily"), "{body}");
        assert!(body.contains("LOCATION:Room B"), "{body}");

        let remote = event("Standup", "Room C");
        assert_eq!(
            Kind::Ical.merge(&base, &local, &remote),
            Merged::Collided(1)
        );
    }

    /// A body no parser reads is reported as what it is, rather than counted
    /// as a disagreement nobody had.
    #[cfg(feature = "dav")]
    #[test]
    fn an_unreadable_body_is_unmergeable_rather_than_collided() {
        let base = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nEND:VCARD\r\n";

        let Merged::Unmergeable(reason) = Kind::Vcard.merge(base, b"not a card", base) else {
            panic!("a body that does not parse is merged");
        };

        assert!(reason.contains("local"), "{reason}");
    }

    /// Mail bodies never change, so its merge is a question with no answer
    /// rather than a silent success.
    #[test]
    fn mail_carries_no_merge() {
        assert!(matches!(
            Kind::Mail.merge(b"a", b"b", b"c"),
            Merged::Unmergeable(_)
        ));
    }
}
