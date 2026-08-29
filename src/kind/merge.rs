//! The three-way merge a run resolves a content conflict with.
//!
//! A conflict is two edits of one item against a base the last sync agreed
//! on. Most of them are not disagreements: one side changed a phone number
//! and the other a note, and the base is what proves it by naming which side
//! touched which field. Merging those needs no one, and reporting them to a
//! person is a background tool asking to be switched off.
//!
//! The merge is built in rather than configured. It is a pure function over
//! bodies the store already holds, there is no taste in it, and the format
//! vocabulary is closed: contacts are vcard-rs, calendars and tasks and
//! journals are ical-rs, and mail is immutable-content and reaches none of
//! this. Because it cannot be swapped it is strictly conservative, resolving
//! on an empty report and on nothing else: a merge nobody can replace has no
//! business deciding anything a person might have decided differently, and
//! the report distinguishes the two exactly.
//!
//! The local body is the merge's left side, so the store's own bytes survive
//! byte for byte and the remote's non-colliding changes are replayed onto
//! them.

#[cfg(feature = "merge")]
use ical::tree::{
    cst::IcalCst,
    merge::{IcalMerge, IcalMergeSide},
};
#[cfg(feature = "merge")]
use vcard::tree::{cst::VcardCst, merge::merge};

use crate::kind::Kind;

/// What a three-way merge concluded about one conflicted item.
// NOTE: a build without the `merge` cargo feature constructs neither
// resolving variant, the merge being the only thing that produces them.
#[cfg_attr(not(feature = "merge"), allow(dead_code))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Merged {
    /// Nobody disagreed: the merged body carries both sides' edits and
    /// resolves the conflict as an ordinary edit.
    Body(Vec<u8>),
    /// Both sides changed the same field, that many times over. No merge
    /// settles it, so the conflict parks for a person.
    Collided(usize),
    /// The merge could not run at all, for the reason named: a body no
    /// parser accepts, or a kind carrying no merge of its own. The conflict
    /// parks untouched, which is what an unanswerable question deserves.
    Unmergeable(String),
}

#[cfg(feature = "merge")]
impl Kind {
    /// Three-way merges the `local` and `remote` bodies of one conflicted
    /// item against the `base` the last sync agreed on.
    ///
    /// The ical merge is told nothing about who the right side speaks for
    /// (RFC 5546 §3.2): neverest syncs a calendar rather than acting as an
    /// attendee, so it makes no such claim and refuses no change on that
    /// ground.
    ///
    /// It prefers the left side, which is the local one for both kinds, and
    /// the preference decides nothing here: a run takes a merge only on an
    /// empty report, so a collision parks rather than being settled by
    /// whoever the preference favours. It is stated rather than defaulted so
    /// that reading this beside tcal, which prefers the right side because
    /// it puts the edit it speaks for there, does not suggest an oversight.
    pub fn merge(self, base: &[u8], local: &[u8], remote: &[u8]) -> Merged {
        match self {
            Self::Mail => Merged::Unmergeable(String::from("mail bodies are immutable")),
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

                let report = merge(&base, &local, &remote);

                match report.conflicts.len() {
                    0 => Merged::Body(report.merged.to_string().into_bytes()),
                    collided => Merged::Collided(collided),
                }
            }
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
                    right_speaks_for: None,
                    prefer: IcalMergeSide::Left,
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

#[cfg(not(feature = "merge"))]
impl Kind {
    /// The merge a build without the `merge` cargo feature cannot run, so
    /// every conflict parks for a person.
    pub fn merge(self, _base: &[u8], _local: &[u8], _remote: &[u8]) -> Merged {
        Merged::Unmergeable(String::from(
            "this build has no three-way merge, rebuild with the merge cargo feature",
        ))
    }
}

/// Names the side whose body no parser accepts, for the log line a parked
/// conflict leaves behind. A body the store holds and cannot read is worth
/// saying out loud rather than counting as a collision.
#[cfg(feature = "merge")]
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
    /// touched which field, so both survive and nobody is asked anything.
    #[cfg(feature = "merge")]
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

    /// The same field set two ways is the residual case no merge settles,
    /// and the one a person is asked about.
    #[cfg(feature = "merge")]
    #[test]
    fn a_same_field_collision_is_not_merged_away() {
        let base = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:+1\r\nEND:VCARD\r\n";
        let local = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:+2\r\nEND:VCARD\r\n";
        let remote = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:+3\r\nEND:VCARD\r\n";

        assert_eq!(Kind::Vcard.merge(base, local, remote), Merged::Collided(1));
    }

    /// The calendar half of the same rule, over the other library.
    #[cfg(feature = "merge")]
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

    /// A body the store holds and no parser reads is reported as what it
    /// is, rather than counted as a disagreement nobody had.
    #[cfg(feature = "merge")]
    #[test]
    fn an_unreadable_body_is_unmergeable_rather_than_collided() {
        let base = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nEND:VCARD\r\n";

        let Merged::Unmergeable(reason) = Kind::Vcard.merge(base, b"not a card", base) else {
            panic!("a body that does not parse is merged");
        };

        assert!(reason.contains("local"), "{reason}");
    }

    /// Mail bodies never change, so a mail item cannot diverge and its
    /// merge is a question with no answer rather than a silent success.
    #[test]
    fn mail_carries_no_merge() {
        assert!(matches!(
            Kind::Mail.merge(b"a", b"b", b"c"),
            Merged::Unmergeable(_)
        ));
    }
}
