//! # Event documents
//!
//! What a Google Calendar or Graph source reads of an iCalendar document
//! before pushing it: the floating times neither provider stores, anchored
//! in a zone, and the occurrences of a series both take apart from its
//! master, each through its instance.

use std::borrow::Cow;

use anyhow::{Context, Result};
use ical::tree::{
    cst::{IcalCst, IcalItem},
    leaf::IcalLeaf,
    line::IcalLine,
    param::node::IcalParamNode,
};
use pimalaya_cli::error::CodedError;

use crate::error::ErrorCode;

/// The date-time properties of an event a floating value is anchored in.
const ANCHORED: &[&str] = &["DTSTART", "DTEND", "RECURRENCE-ID", "EXDATE", "RDATE"];

/// The occurrences a series document sets apart from its master.
#[derive(Debug, Default)]
pub struct Occurrences {
    /// The `EXDATE` values of the master, each an occurrence it cancels.
    pub excluded: Vec<String>,
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

            let Some(recurrence_id) = lines(event, "RECURRENCE-ID").next() else {
                for line in lines(event, "EXDATE") {
                    occurrences.excluded.extend(values(line));
                }
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

            let mut alone = calendar.clone();
            alone.items = calendar
                .items
                .iter()
                .enumerate()
                .filter(|(other, item)| *other == index || !is_event(item))
                .map(|(_, item)| item.clone())
                .collect();

            occurrences.overrides.push(Override {
                recurrence_id: recurrence_id.raw_value_str().trim().to_owned(),
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
    /// The `RECURRENCE-ID` value, parameters aside.
    pub recurrence_id: String,
    /// Whether it cancels the occurrence (`STATUS:CANCELLED`).
    pub cancelled: bool,
    /// Whether it overrides every later occurrence too.
    pub this_and_future: bool,
    /// The document holding this component as its only event.
    pub document: Vec<u8>,
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

        assert_eq!(occurrences.excluded, ["20261125T140000"]);
        assert_eq!(occurrences.overrides.len(), 1);
        let moved = &occurrences.overrides[0];
        assert_eq!(moved.recurrence_id, "20261124T140000");
        assert!(!moved.cancelled && !moved.this_and_future);

        let document = String::from_utf8(moved.document.clone()).unwrap();
        assert_eq!(document.matches("BEGIN:VEVENT").count(), 1, "{document}");
        assert!(document.contains("SUMMARY:Daily, moved"), "{document}");
        assert!(document.contains("PRODID:-//test//EN"), "{document}");
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
