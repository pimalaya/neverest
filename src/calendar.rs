//! # Event documents
//!
//! What a Google Calendar or Graph source reads of an iCalendar document
//! before pushing it: the floating times neither provider stores, anchored
//! in a zone, and the occurrences of a series both take apart from its
//! master, each through its instance, found by when it originally starts.

use std::{borrow::Cow, fmt};

use anyhow::{Context, Result};
use ical::tree::{
    cst::{IcalCst, IcalItem},
    leaf::IcalLeaf,
    line::IcalLine,
    param::node::IcalParamNode,
};
use jiff::{
    Timestamp,
    civil::{Date, DateTime},
    tz::TimeZone,
};
use pimalaya_cli::error::CodedError;

use crate::error::ErrorCode;

/// The date-time properties of an event a floating value is anchored in.
const ANCHORED: &[&str] = &["DTSTART", "DTEND", "RECURRENCE-ID", "EXDATE", "RDATE"];

/// The occurrences a series document sets apart from its master.
#[derive(Debug, Default)]
pub struct Occurrences {
    /// The document holding the master as its only event, the series an
    /// override is read against; `None` for overrides with no master.
    pub master: Option<Vec<u8>>,
    /// The `EXDATE` values of the master, each an occurrence it cancels.
    pub excluded: Vec<OccurrenceId>,
    /// The components overriding an occurrence.
    pub overrides: Vec<Override>,
}

impl Occurrences {
    /// Reads the occurrences `ical` sets apart from its master.
    pub fn read(ical: &[u8]) -> Result<Self> {
        let calendar = IcalCst::parse(ical).context("Parse the iCalendar object error")?;
        let mut occurrences = Self::default();

        for (index, item) in calendar.items.iter().enumerate() {
            let IcalItem::Component(event) = item else {
                continue;
            };
            if !is_named(event, "VEVENT") {
                continue;
            }

            let mut alone = calendar.clone();
            alone.items = calendar
                .items
                .iter()
                .enumerate()
                .filter(|(other, item)| *other == index || !is_event(item))
                .map(|(_, item)| item.clone())
                .collect();

            let Some(recurrence_id) = lines(event, "RECURRENCE-ID").next() else {
                for line in lines(event, "EXDATE") {
                    let zone = zone(line);
                    occurrences
                        .excluded
                        .extend(values(line).into_iter().map(|value| OccurrenceId {
                            value,
                            zone: zone.clone(),
                        }));
                }
                occurrences.master.get_or_insert(alone.to_bytes());
                continue;
            };

            let cancelled = lines(event, "STATUS").any(|line| {
                line.raw_value_str()
                    .trim()
                    .eq_ignore_ascii_case("CANCELLED")
            });
            let this_and_future = recurrence_id.params.iter().any(|param| {
                param.name.get().eq_ignore_ascii_case("RANGE")
                    && param
                        .values
                        .iter()
                        .any(|value| value.get().eq_ignore_ascii_case("THISANDFUTURE"))
            });

            occurrences.overrides.push(Override {
                recurrence_id: OccurrenceId {
                    value: recurrence_id.raw_value_str().trim().to_owned(),
                    zone: zone(recurrence_id),
                },
                cancelled,
                this_and_future,
                document: alone.to_bytes(),
            });
        }

        Ok(occurrences)
    }

    /// Whether the document sets no occurrence apart.
    #[cfg(feature = "msgraph")]
    pub fn is_empty(&self) -> bool {
        self.excluded.is_empty() && self.overrides.is_empty()
    }

    /// Refuses an override of an occurrence and every later one
    /// (`RANGE=THISANDFUTURE`), which `provider` takes through no instance.
    pub fn refuse_ranges(&self, provider: &str) -> Result<()> {
        match self.overrides.iter().find(|o| o.this_and_future) {
            Some(ranged) => {
                let message = format!(
                    "{provider} cannot take the override of {} and every later \
                     occurrence (RANGE=THISANDFUTURE)",
                    ranged.recurrence_id
                );
                Err(CodedError::new(ErrorCode::Unsupported, message).into())
            }
            None => Ok(()),
        }
    }
}

/// One component of a series overriding an occurrence.
#[derive(Debug)]
pub struct Override {
    /// The occurrence its `RECURRENCE-ID` names.
    pub recurrence_id: OccurrenceId,
    /// Whether it cancels the occurrence (`STATUS:CANCELLED`).
    pub cancelled: bool,
    /// Whether it overrides every later occurrence too.
    pub this_and_future: bool,
    /// The document holding this component as its only event.
    pub document: Vec<u8>,
}

/// An occurrence of a series, named by its original start as a
/// `RECURRENCE-ID` or an `EXDATE` value names it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OccurrenceId {
    /// The value, parameters aside: a date, a wall time or a UTC stamp.
    pub value: String,
    /// The zone its `TZID` names, which a wall time is read in.
    pub zone: Option<String>,
}

/// When an occurrence originally starts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OriginalStart {
    /// The date of an all-day occurrence.
    Date(Date),
    /// The instant of a timed occurrence: a UTC stamp, or a wall time in a
    /// zone the time zone database names.
    Instant(Timestamp),
    /// A wall time in no zone known: floating, or in a `TZID` the time zone
    /// database does not name.
    Wall(DateTime),
}

impl OccurrenceId {
    /// The occurrence `value` names in no zone known, as an intent names
    /// one in the form of its series' `DTSTART` (pimdir STORAGE Annex B.2).
    pub fn bare(value: &str) -> Self {
        Self {
            value: value.trim().to_owned(),
            zone: None,
        }
    }

    /// When the occurrence originally starts; `None` for a value of no
    /// such shape.
    pub fn start(&self) -> Option<OriginalStart> {
        let value = self.value.as_str();
        if value.len() == 8 {
            return Date::strptime("%Y%m%d", value)
                .ok()
                .map(OriginalStart::Date);
        }

        let (wall, utc) = match value.strip_suffix(['Z', 'z']) {
            Some(wall) => (wall, true),
            None => (value, false),
        };
        let wall = DateTime::strptime("%Y%m%dT%H%M%S", wall).ok()?;
        let zone = match utc {
            true => Some(TimeZone::UTC),
            false => self
                .zone
                .as_deref()
                .and_then(|zone| TimeZone::get(zone).ok()),
        };

        match zone {
            Some(zone) => Some(OriginalStart::Instant(
                wall.to_zoned_datetime(zone).ok()?.timestamp(),
            )),
            None => Some(OriginalStart::Wall(wall)),
        }
    }

    /// Whether it names the timed occurrence originally starting at
    /// `original`, a wall time of no zone known read in `zone`, the series'.
    pub fn starts_at(&self, original: Timestamp, zone: Option<&str>) -> bool {
        match self.start() {
            Some(OriginalStart::Instant(instant)) => instant == original,
            Some(OriginalStart::Wall(wall)) => zone
                .and_then(|zone| TimeZone::get(zone).ok())
                .is_some_and(|zone| original.to_zoned_datetime(zone).datetime() == wall),
            _ => false,
        }
    }
}

impl fmt::Display for OccurrenceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.value)
    }
}

/// Whether an event of `ical` holds a floating date-time.
pub fn has_floating(ical: &[u8]) -> Result<bool> {
    let calendar = IcalCst::parse(ical).context("Parse the iCalendar object error")?;
    Ok(events(&calendar).any(|event| event.items.iter().any(is_floating)))
}

/// `ical` with every floating date-time of its events anchored in `zone`, a
/// `TZID`, or in UTC, a `Z`, when `None`.
pub fn anchor_floating(ical: &[u8], zone: Option<&str>) -> Result<Vec<u8>> {
    let mut calendar = IcalCst::parse(ical).context("Parse the iCalendar object error")?;

    for item in &mut calendar.items {
        let IcalItem::Component(event) = item else {
            continue;
        };
        if !is_named(event, "VEVENT") {
            continue;
        }

        let floating_start = event.items.iter().any(|item| {
            is_floating(item)
                && matches!(item, IcalItem::Prop(line) if line.name.get().eq_ignore_ascii_case("DTSTART"))
        });
        if floating_start {
            for item in &mut event.items {
                if let IcalItem::Prop(line) = item {
                    anchor_until(line, zone);
                }
            }
        }

        for item in &mut event.items {
            if !is_floating(item) {
                continue;
            }
            let IcalItem::Prop(line) = item else {
                continue;
            };

            match zone {
                Some(zone) => line.params.insert(
                    0,
                    IcalParamNode {
                        name: IcalLeaf(Cow::Borrowed("TZID")),
                        values: vec![IcalLeaf(Cow::Owned(zone.to_owned()))],
                        escaper: Default::default(),
                    },
                ),
                None => {
                    let utc: Vec<String> =
                        values(line).into_iter().map(|value| value + "Z").collect();
                    line.value.set(&utc);
                }
            }
        }
    }

    Ok(calendar.to_bytes())
}

/// Rewrites the floating `UNTIL` of a recurrence rule as the UTC instant its
/// wall time names in `zone`, UTC when `None`.
///
/// A series anchored in a zone must bound its rule in UTC (RFC 5545 §3.3.10):
/// left floating, a provider refuses the rule or reads it in another zone.
fn anchor_until(line: &mut IcalLine<'_>, zone: Option<&str>) {
    let name = line.name.get();
    if !name.eq_ignore_ascii_case("RRULE") && !name.eq_ignore_ascii_case("EXRULE") {
        return;
    }

    let raw = line.raw_value_str().into_owned();
    let Some((index, wall)) = raw.split(';').enumerate().find_map(|(index, part)| {
        let (key, value) = part.split_once('=')?;
        let value = value.trim();
        (key.trim().eq_ignore_ascii_case("UNTIL") && is_wall_time(value))
            .then(|| (index, value.to_owned()))
    }) else {
        return;
    };

    let Ok(wall) = DateTime::strptime("%Y%m%dT%H%M%S", &wall) else {
        return;
    };
    let tz = match zone {
        Some(zone) => match TimeZone::get(zone) {
            Ok(tz) => tz,
            Err(_) => return,
        },
        None => TimeZone::UTC,
    };
    let Ok(zoned) = wall.to_zoned_datetime(tz) else {
        return;
    };
    let until = zoned
        .timestamp()
        .strftime("UNTIL=%Y%m%dT%H%M%SZ")
        .to_string();
    line.value.set_component(index, &[until]);
}

/// The events of a calendar.
fn events<'c, 'a>(calendar: &'c IcalCst<'a>) -> impl Iterator<Item = &'c IcalCst<'a>> {
    calendar.items.iter().filter_map(|item| match item {
        IcalItem::Component(event) if is_named(event, "VEVENT") => Some(event.as_ref()),
        _ => None,
    })
}

/// Whether a calendar item is an event.
fn is_event(item: &IcalItem<'_>) -> bool {
    matches!(item, IcalItem::Component(event) if is_named(event, "VEVENT"))
}

/// Whether a component is named `name`.
fn is_named(component: &IcalCst<'_>, name: &str) -> bool {
    component
        .begin
        .as_ref()
        .is_some_and(|begin| begin.raw_value_str().trim().eq_ignore_ascii_case(name))
}

/// The property lines of a component named `name`.
fn lines<'c, 'a>(
    component: &'c IcalCst<'a>,
    name: &'c str,
) -> impl Iterator<Item = &'c IcalLine<'a>> {
    component.items.iter().filter_map(move |item| match item {
        IcalItem::Prop(line) if line.name.get().eq_ignore_ascii_case(name) => Some(line),
        _ => None,
    })
}

/// The values of a list property, such as `EXDATE`.
fn values(line: &IcalLine<'_>) -> Vec<String> {
    line.raw_value_str()
        .split(',')
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect()
}

/// The zone a property's `TZID` names.
fn zone(line: &IcalLine<'_>) -> Option<String> {
    line.params
        .iter()
        .find(|param| param.name.get().eq_ignore_ascii_case("TZID"))
        .and_then(|param| param.values.first())
        .map(|value| value.get().trim().trim_matches('"').to_owned())
        .filter(|zone| !zone.is_empty())
}

/// Whether an item is a date-time property of [`ANCHORED`] naming neither a
/// zone nor UTC.
fn is_floating(item: &IcalItem<'_>) -> bool {
    let IcalItem::Prop(line) = item else {
        return false;
    };
    if !ANCHORED
        .iter()
        .any(|name| line.name.get().eq_ignore_ascii_case(name))
    {
        return false;
    }

    let typed = line.params.iter().any(|param| {
        let name = param.name.get();
        name.eq_ignore_ascii_case("TZID")
            || (name.eq_ignore_ascii_case("VALUE")
                && !param
                    .values
                    .iter()
                    .all(|value| value.get().eq_ignore_ascii_case("DATE-TIME")))
    });
    if typed {
        return false;
    }

    let values = values(line);
    !values.is_empty() && values.iter().all(|value| is_wall_time(value))
}

/// Whether a value is a date-time with no `Z`: `YYYYMMDDTHHMMSS`.
fn is_wall_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 15
        && bytes[8] == b'T'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 8 || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERIES: &str = "BEGIN:VCALENDAR\r\n\
        VERSION:2.0\r\n\
        PRODID:-//test//EN\r\n\
        BEGIN:VEVENT\r\n\
        UID:series\r\n\
        DTSTART;TZID=Europe/Paris:20261123T140000\r\n\
        DTEND;TZID=Europe/Paris:20261123T150000\r\n\
        RRULE:FREQ=DAILY;COUNT=4\r\n\
        EXDATE;TZID=Europe/Paris:20261125T140000\r\n\
        SUMMARY:Daily\r\n\
        END:VEVENT\r\n\
        BEGIN:VEVENT\r\n\
        UID:series\r\n\
        RECURRENCE-ID;TZID=Europe/Paris:20261124T140000\r\n\
        DTSTART;TZID=Europe/Paris:20261124T160000\r\n\
        DTEND;TZID=Europe/Paris:20261124T170000\r\n\
        SUMMARY:Daily, moved\r\n\
        END:VEVENT\r\n\
        END:VCALENDAR\r\n";

    #[test]
    fn a_series_sets_its_exclusion_and_its_override_apart() {
        let occurrences = Occurrences::read(SERIES.as_bytes()).unwrap();

        let paris = |value: &str| OccurrenceId {
            value: value.into(),
            zone: Some("Europe/Paris".into()),
        };
        assert_eq!(occurrences.excluded, [paris("20261125T140000")]);
        assert_eq!(occurrences.overrides.len(), 1);
        let moved = &occurrences.overrides[0];
        assert_eq!(moved.recurrence_id, paris("20261124T140000"));
        assert!(!moved.cancelled && !moved.this_and_future);

        let document = String::from_utf8(moved.document.clone()).unwrap();
        assert_eq!(document.matches("BEGIN:VEVENT").count(), 1, "{document}");
        assert!(document.contains("SUMMARY:Daily, moved"), "{document}");
        assert!(document.contains("PRODID:-//test//EN"), "{document}");

        let master = String::from_utf8(occurrences.master.unwrap()).unwrap();
        assert_eq!(master.matches("BEGIN:VEVENT").count(), 1, "{master}");
        assert!(master.contains("RRULE:FREQ=DAILY"), "{master}");
    }

    /// An occurrence is named by when it starts.
    ///
    /// A date, a wall time read in its `TZID`, a UTC stamp, or a wall time in
    /// no zone known, read in the series' zone.
    #[test]
    fn an_occurrence_starts_at_the_instant_its_zone_reads() {
        let nine_utc: Timestamp = "2026-12-21T09:00:00Z".parse().unwrap();
        let paris = OccurrenceId {
            value: "20261221T100000".into(),
            zone: Some("Europe/Paris".into()),
        };
        assert!(paris.starts_at(nine_utc, None));
        assert!(!paris.starts_at("2026-12-21T10:00:00Z".parse().unwrap(), None));

        assert!(OccurrenceId::bare("20261221T090000Z").starts_at(nine_utc, None));

        let bare = OccurrenceId::bare("20261221T100000");
        assert!(bare.starts_at(nine_utc, Some("Europe/Paris")));
        assert!(!bare.starts_at(nine_utc, None));
        assert!(!bare.starts_at(nine_utc, Some("UTC")));

        let unknown = OccurrenceId {
            zone: Some("Custom Zone".into()),
            ..bare.clone()
        };
        assert_eq!(
            unknown.start(),
            Some(OriginalStart::Wall("2026-12-21T10:00:00".parse().unwrap()))
        );

        let date = OccurrenceId::bare("20261221").start();
        assert_eq!(
            date,
            Some(OriginalStart::Date("2026-12-21".parse().unwrap()))
        );
        assert_eq!(OccurrenceId::bare("next monday").start(), None);
    }

    #[test]
    fn an_override_of_every_later_occurrence_is_refused() {
        let ranged = SERIES.replace(
            "RECURRENCE-ID;TZID",
            "RECURRENCE-ID;RANGE=THISANDFUTURE;TZID",
        );
        let occurrences = Occurrences::read(ranged.as_bytes()).unwrap();

        let err = occurrences.refuse_ranges("Google").unwrap_err();
        let code = pimalaya_cli::error::code_of::<ErrorCode>(&err);
        assert_eq!(code, Some(ErrorCode::Unsupported), "{err:#}");
    }

    const FLOATING: &str = "BEGIN:VCALENDAR\r\n\
        VERSION:2.0\r\n\
        BEGIN:VEVENT\r\n\
        UID:floating\r\n\
        DTSTART:20261126T100000\r\n\
        DTEND:20261126T110000\r\n\
        DTSTAMP:20261115T120000Z\r\n\
        SUMMARY:Floating\r\n\
        END:VEVENT\r\n\
        END:VCALENDAR\r\n";

    #[test]
    fn a_floating_time_is_anchored_in_the_zone_given() {
        assert!(has_floating(FLOATING.as_bytes()).unwrap());

        let anchored = anchor_floating(FLOATING.as_bytes(), Some("Europe/Paris")).unwrap();
        let anchored = String::from_utf8(anchored).unwrap();

        assert!(
            anchored.contains("DTSTART;TZID=Europe/Paris:20261126T100000\r\n"),
            "{anchored}"
        );
        assert!(
            anchored.contains("DTEND;TZID=Europe/Paris:20261126T110000\r\n"),
            "{anchored}"
        );
        assert!(
            anchored.contains("DTSTAMP:20261115T120000Z\r\n"),
            "{anchored}"
        );
        assert!(!has_floating(anchored.as_bytes()).unwrap());
    }

    /// A floating series anchored in a zone bounds its rule in UTC, the
    /// wall time of its `UNTIL` read in that zone (RFC 5545 §3.3.10).
    ///
    /// Left floating beside a `TZID` start, the rule is invalid.
    #[test]
    fn a_floating_series_bounds_its_rule_in_utc() {
        let series = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:s1\r\n\
                      DTSTAMP:20261115T120000Z\r\nDTSTART:20261126T100000\r\n\
                      RRULE:FREQ=WEEKLY;BYDAY=TH,FR;UNTIL=20261231T100000\r\n\
                      END:VEVENT\r\nEND:VCALENDAR\r\n";

        let anchored = anchor_floating(series.as_bytes(), Some("Europe/Paris")).unwrap();
        let anchored = String::from_utf8(anchored).unwrap();
        assert!(
            anchored.contains("RRULE:FREQ=WEEKLY;BYDAY=TH,FR;UNTIL=20261231T090000Z\r\n"),
            "{anchored}"
        );

        let anchored = anchor_floating(series.as_bytes(), None).unwrap();
        let anchored = String::from_utf8(anchored).unwrap();
        assert!(
            anchored.contains("RRULE:FREQ=WEEKLY;BYDAY=TH,FR;UNTIL=20261231T100000Z\r\n"),
            "{anchored}"
        );
    }

    #[test]
    fn a_floating_time_is_anchored_in_utc_without_a_zone() {
        let anchored = anchor_floating(FLOATING.as_bytes(), None).unwrap();
        let anchored = String::from_utf8(anchored).unwrap();

        assert!(
            anchored.contains("DTSTART:20261126T100000Z\r\n"),
            "{anchored}"
        );
        assert!(
            anchored.contains("DTEND:20261126T110000Z\r\n"),
            "{anchored}"
        );
    }

    #[test]
    fn a_date_a_zone_or_utc_is_no_floating_time() {
        let anchored = [
            "DTSTART;VALUE=DATE:20261126",
            "DTSTART;TZID=Europe/Paris:20261126T100000",
            "DTSTART:20261126T100000Z",
        ];
        for start in anchored {
            let ical = SERIES.replace("DTSTART;TZID=Europe/Paris:20261123T140000", start);
            let ical = ical.replace("DTSTART;TZID=Europe/Paris:20261124T160000", start);
            assert!(!has_floating(ical.as_bytes()).unwrap(), "{start}");
        }
    }
}
