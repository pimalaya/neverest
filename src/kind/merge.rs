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

#[cfg(any(
    feature = "dav",
    feature = "msgraph",
    feature = "gpeople",
    feature = "gcal"
))]
use ical::tree::{cst::IcalCst, merge::IcalMerge};
#[cfg(any(
    feature = "dav",
    feature = "msgraph",
    feature = "gpeople",
    feature = "gcal"
))]
use vcard::tree::{cst::VcardCst, merge::VcardMerge};

use crate::kind::Kind;

/// What a three-way merge concluded about one conflicted item.
// NOTE: a build without `dav` carries no mutable-content kind, so mail is
// the only arm left and neither resolving variant is ever constructed.
#[cfg_attr(
    not(any(
        feature = "dav",
        feature = "msgraph",
        feature = "gpeople",
        feature = "gcal"
    )),
    allow(dead_code)
)]
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
    /// Three-way merges `local` and `remote` against the `base` last agreed on.
    ///
    /// The ical merge names no attendee the right side speaks for (RFC 5546
    /// §3.2): neverest syncs a calendar rather than acting as one. Preferring
    /// left decides nothing here, a run merging only on an empty report.
    // NOTE: without `dav` the mail arm is the whole match, and mail is
    // immutable-content, so no side is ever read.
    #[cfg_attr(
        not(any(
            feature = "dav",
            feature = "msgraph",
            feature = "gpeople",
            feature = "gcal"
        )),
        allow(unused_variables)
    )]
    pub fn merge(self, base: &[u8], local: &[u8], remote: &[u8]) -> Merged {
        match self {
            Self::Mail => Merged::Unmergeable(String::from("mail bodies are immutable")),
            #[cfg(any(
                feature = "dav",
                feature = "msgraph",
                feature = "gpeople",
                feature = "gcal"
            ))]
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
            #[cfg(any(
                feature = "dav",
                feature = "msgraph",
                feature = "gpeople",
                feature = "gcal"
            ))]
            Self::Ical => {
                // NOTE: the stamps say when a side last wrote, not what
                // anybody decided, so only the local side keeps them.
                let remote_bytes = remote;
                let base = without_stamps(base);
                let remote = without_stamps(remote);
                let (base, local, remote) = match (
                    IcalCst::parse(&base),
                    IcalCst::parse(local),
                    IcalCst::parse(&remote),
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
                    0 => {
                        let merged = report.merged.to_string().into_bytes();
                        Merged::Body(with_sequences_of(&merged, remote_bytes))
                    }
                    collided => Merged::Collided(collided),
                }
            }
        }
    }
}

/// The properties a calendar writer stamps on every write: when, and how
/// many times, never what. A server rewrites them on its own (Graph moves
/// `LAST-MODIFIED` when it sends a meeting's invitations, and carries no
/// `SEQUENCE`), so two sides touching them is no disagreement.
#[cfg(any(
    feature = "dav",
    feature = "msgraph",
    feature = "gpeople",
    feature = "gcal"
))]
const STAMPS: &[&str] = &["DTSTAMP", "LAST-MODIFIED", "CREATED", "SEQUENCE"];

/// The name of an unfolded content line, upper-cased.
#[cfg(any(
    feature = "dav",
    feature = "msgraph",
    feature = "gpeople",
    feature = "gcal"
))]
fn line_name(line: &[u8]) -> String {
    let end = line
        .iter()
        .position(|byte| *byte == b';' || *byte == b':')
        .unwrap_or(line.len());
    String::from_utf8_lossy(&line[..end]).to_ascii_uppercase()
}

/// Splits a body into its logical lines, each with its folds and its
/// ending, so a line is dropped or replaced whole.
#[cfg(any(
    feature = "dav",
    feature = "msgraph",
    feature = "gpeople",
    feature = "gcal"
))]
fn logical_lines(body: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while at < body.len() {
        if body[at] == b'\n' {
            let next = at + 1;
            let folded = matches!(body.get(next), Some(b' ' | b'\t'));
            if !folded {
                lines.push(&body[start..next]);
                start = next;
            }
        }
        at += 1;
    }
    if start < body.len() {
        lines.push(&body[start..]);
    }
    lines
}

/// The body without its stamps, in every component.
#[cfg(any(
    feature = "dav",
    feature = "msgraph",
    feature = "gpeople",
    feature = "gcal"
))]
fn without_stamps(body: &[u8]) -> Vec<u8> {
    logical_lines(body)
        .into_iter()
        .filter(|line| !STAMPS.contains(&line_name(line).as_str()))
        .flatten()
        .copied()
        .collect()
}

/// The `SEQUENCE` of each component of a body, keyed by its `UID` and
/// `RECURRENCE-ID` lines as written, and the component's position.
#[cfg(any(
    feature = "dav",
    feature = "msgraph",
    feature = "gpeople",
    feature = "gcal"
))]
fn sequences(body: &[u8]) -> Vec<(String, Option<u64>)> {
    let mut found = Vec::new();
    let mut key: Option<String> = None;
    let mut sequence = None;
    for line in logical_lines(body) {
        let name = line_name(line);
        let value = || {
            let text = String::from_utf8_lossy(line);
            let unfolded = text.replace("\r\n ", "").replace("\r\n\t", "");
            unfolded
                .split_once(':')
                .map(|(_, value)| value.trim().to_string())
                .unwrap_or_default()
        };
        match name.as_str() {
            "BEGIN" if matches!(value().as_str(), "VEVENT" | "VTODO" | "VJOURNAL") => {
                key = Some(String::new());
                sequence = None;
            }
            "UID" | "RECURRENCE-ID" if key.is_some() => {
                if let Some(key) = key.as_mut() {
                    key.push_str(&name);
                    key.push('=');
                    key.push_str(&value());
                    key.push('\n');
                }
            }
            "SEQUENCE" if key.is_some() => sequence = value().parse().ok(),
            "END" if matches!(value().as_str(), "VEVENT" | "VTODO" | "VJOURNAL") => {
                if let Some(key) = key.take() {
                    found.push((key, sequence));
                }
            }
            _ => {}
        }
    }
    found
}

/// The merged body with each component's `SEQUENCE` raised to the
/// remote's where the remote counted further: a revision count only grows,
/// and the stamps left the merge, so neither side's count is lost.
#[cfg(any(
    feature = "dav",
    feature = "msgraph",
    feature = "gpeople",
    feature = "gcal"
))]
fn with_sequences_of(merged: &[u8], remote: &[u8]) -> Vec<u8> {
    let remote = sequences(remote);
    let higher = |key: &str, current: Option<u64>| -> Option<u64> {
        let theirs = remote
            .iter()
            .find(|(other, _)| other == key)
            .and_then(|(_, sequence)| *sequence)?;
        (current.is_none_or(|ours| theirs > ours)).then_some(theirs)
    };
    if remote.iter().all(|(_, sequence)| sequence.is_none()) {
        return merged.to_vec();
    }

    // NOTE: a component is rewritten only where the remote counted
    // further, so a body the remote left alone keeps its bytes.
    let mut out = Vec::with_capacity(merged.len());
    let mut component: Vec<&[u8]> = Vec::new();
    let mut inside = false;
    for line in logical_lines(merged) {
        let name = line_name(line);
        let opens = name == "BEGIN" && is_dated_component(line);
        let closes = name == "END" && is_dated_component(line);
        if opens {
            inside = true;
            component.clear();
        }
        if !inside {
            out.extend_from_slice(line);
            continue;
        }
        component.push(line);
        if closes {
            inside = false;
            let joined: Vec<u8> = component
                .iter()
                .flat_map(|line| line.iter())
                .copied()
                .collect();
            let (key, current) = sequences(&joined).pop().unwrap_or_default();
            match higher(&key, current) {
                None => out.extend_from_slice(&joined),
                Some(sequence) => {
                    let ending: &[u8] = if component[0].ends_with(b"\r\n") {
                        b"\r\n"
                    } else {
                        b"\n"
                    };
                    let mut written = false;
                    for line in &component {
                        if line_name(line) == "SEQUENCE" {
                            out.extend_from_slice(format!("SEQUENCE:{sequence}").as_bytes());
                            out.extend_from_slice(ending);
                            written = true;
                        } else if line_name(line) == "END" && !written {
                            out.extend_from_slice(format!("SEQUENCE:{sequence}").as_bytes());
                            out.extend_from_slice(ending);
                            out.extend_from_slice(line);
                        } else {
                            out.extend_from_slice(line);
                        }
                    }
                }
            }
        }
    }
    out
}

/// Whether a `BEGIN` or `END` line opens or closes an event, a to-do or
/// a journal entry.
#[cfg(any(
    feature = "dav",
    feature = "msgraph",
    feature = "gpeople",
    feature = "gcal"
))]
fn is_dated_component(line: &[u8]) -> bool {
    let text = String::from_utf8_lossy(line).to_ascii_uppercase();
    let value = text.split_once(':').map_or("", |(_, value)| value.trim());
    matches!(value, "VEVENT" | "VTODO" | "VJOURNAL")
}

/// Names the side whose body no parser accepts, for the log line a parked
/// conflict leaves behind, rather than counting it as a collision.
#[cfg(any(
    feature = "dav",
    feature = "msgraph",
    feature = "gpeople",
    feature = "gcal"
))]
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
    #[cfg(any(
        feature = "dav",
        feature = "msgraph",
        feature = "gpeople",
        feature = "gcal"
    ))]
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
    #[cfg(any(
        feature = "dav",
        feature = "msgraph",
        feature = "gpeople",
        feature = "gcal"
    ))]
    #[test]
    fn a_same_field_collision_is_not_merged_away() {
        let base = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:+1\r\nEND:VCARD\r\n";
        let local = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:+2\r\nEND:VCARD\r\n";
        let remote = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Jane Doe\r\nTEL:+3\r\nEND:VCARD\r\n";

        assert_eq!(Kind::Vcard.merge(base, local, remote), Merged::Collided(1));
    }

    /// The calendar half of the same rule, over the other library.
    #[cfg(any(
        feature = "dav",
        feature = "msgraph",
        feature = "gpeople",
        feature = "gcal"
    ))]
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

    /// What MOA saw on Graph: a meeting created, then edited before the
    /// next run, while Graph restamped its own copy on sending the
    /// invitations. Only the summary was decided, by one side.
    #[cfg(any(
        feature = "dav",
        feature = "msgraph",
        feature = "gpeople",
        feature = "gcal"
    ))]
    #[test]
    fn stamps_rewritten_on_both_sides_do_not_collide() {
        let base = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:m1\r\n\
            DTSTAMP:20261003T210000Z\r\nSEQUENCE:0\r\nSUMMARY:Review\r\n\
            END:VEVENT\r\nEND:VCALENDAR\r\n";
        let local = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:m1\r\n\
            DTSTAMP:20261003T210500Z\r\nSEQUENCE:1\r\nSUMMARY:Review (moved)\r\n\
            END:VEVENT\r\nEND:VCALENDAR\r\n";
        let remote = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:m1\r\n\
            DTSTAMP:20261003T210112Z\r\nCREATED:20261003T210001Z\r\n\
            LAST-MODIFIED:20261003T210112Z\r\nSUMMARY:Review\r\n\
            X-MSGRAPH-WEB-LINK:https://outlook.x.org/e1\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

        let Merged::Body(body) = Kind::Ical.merge(base, local, remote) else {
            panic!("restamped sides collide");
        };

        let body = String::from_utf8(body).unwrap();
        assert!(body.contains("SUMMARY:Review (moved)\r\n"), "{body}");
        assert!(body.contains("SEQUENCE:1\r\n"), "{body}");
        assert!(body.contains("DTSTAMP:20261003T210500Z\r\n"), "{body}");
        assert!(
            body.contains("X-MSGRAPH-WEB-LINK:https://outlook.x.org/e1"),
            "{body}"
        );
    }

    /// A revision count only grows: the remote counted further, so the
    /// merged event carries its count, and only that line changes.
    #[cfg(any(
        feature = "dav",
        feature = "msgraph",
        feature = "gpeople",
        feature = "gcal"
    ))]
    #[test]
    fn a_merged_event_keeps_the_higher_sequence() {
        let event = |sequence: u8, summary: &str, location: &str| {
            format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:e1\r\n\
                 SEQUENCE:{sequence}\r\nSUMMARY:{summary}\r\nLOCATION:{location}\r\n\
                 END:VEVENT\r\nEND:VCALENDAR\r\n"
            )
            .into_bytes()
        };

        let base = event(1, "Standup", "Room A");
        let local = event(2, "Standup", "Room B");
        let remote = event(3, "Daily", "Room A");

        let Merged::Body(body) = Kind::Ical.merge(&base, &local, &remote) else {
            panic!("disjoint edits collide");
        };
        let body = String::from_utf8(body).unwrap();
        assert_eq!(
            body,
            String::from_utf8(event(3, "Daily", "Room B")).unwrap()
        );

        // NOTE: a real collision is still one.
        let remote = event(3, "Standup", "Room C");
        assert_eq!(
            Kind::Ical.merge(&base, &local, &remote),
            Merged::Collided(1)
        );
    }

    /// A body no parser reads is reported as what it is, rather than counted
    /// as a disagreement nobody had.
    #[cfg(any(
        feature = "dav",
        feature = "msgraph",
        feature = "gpeople",
        feature = "gcal"
    ))]
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
