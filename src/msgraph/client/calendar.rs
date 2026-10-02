//! # Graph calendars
//!
//! The calendar half of [`GraphClient`]: the user's calendars as
//! collections, events as iCalendar through io-msgraph's `ical` projection.
//!
//! Graph's event delta only runs over a calendar view, a time window, and
//! an event that leaves a sliding window would read as deleted and be
//! deleted on every other source. So the enumeration lists a calendar's
//! lone events and series masters in full every run, with no window, the
//! master's `changeKey` as the revision; a full listing reports removals by
//! absence. A series is one item, its exceptions read with it from the
//! instances of its own date range.
//!
//! Graph has no conditional write for events, so an `If-Match` from the
//! engine is checked against the server's current revision right before
//! the write. Only the series master is written: an exception edited
//! locally does not push.

use std::io::{Read, Write};

use anyhow::{Context, Result, bail};
use io_msgraph::v1::{
    rest::users::{
        calendars::list::{MsgraphCalendarsListParams, MsgraphCalendarsListResponse},
        events::{
            MsgraphEvent, MsgraphEventType,
            ical::{MSGRAPH_EVENT_ICAL_SELECT, MSGRAPH_EVENT_STASH_EXPAND},
            list::MsgraphEventsListParams,
        },
    },
    send::MsgraphSend,
};
use jiff::{Timestamp, ToSpan, civil::Date, tz::TimeZone};
use log::{debug, trace, warn};
use url::Url;

use super::GraphClient;
use crate::{
    client::{EnumEntry, Enumeration, WrittenItem},
    item::collection::Collection,
};

/// The `$select` of the enumeration: what tells an event's identity, its
/// kind and its revision.
const LIST_SELECT: &str = "id,changeKey,type,seriesMasterId";

/// The page size requested when listing events.
const PAGE_SIZE: u32 = 500;

/// How far past its start an open-ended series is searched for exceptions.
const OPEN_SERIES_YEARS: i16 = 5;

impl GraphClient {
    /// Lists the user's calendars, keyed by id and named by their name.
    pub(super) fn list_calendars(&mut self) -> Result<Vec<Collection>> {
        debug!("begin graph calendar listing");

        let params = MsgraphCalendarsListParams {
            top: Some(100),
            ..Default::default()
        };
        let mut page = self
            .op(|graph| graph.calendars_list(&params))
            .context("List calendars error")?;

        let mut collections = Vec::new();
        loop {
            for calendar in page.value {
                if calendar.id.is_empty() {
                    continue;
                }
                let name = calendar
                    .name
                    .as_option()
                    .cloned()
                    .unwrap_or_else(|| calendar.id.clone());
                collections.push(Collection {
                    id: calendar.id,
                    name,
                    total: None,
                    unread: None,
                });
            }

            let Some(next) = page.next_link else {
                break;
            };
            let url = Url::parse(&next).context("Cannot parse the calendar paging link")?;
            page = self
                .op(|graph| {
                    let coroutine =
                        MsgraphSend::<MsgraphCalendarsListResponse>::get(&graph.auth, url);
                    graph.run(coroutine)
                })
                .context("Follow calendar paging link error")?;
        }

        debug!("end of graph calendar listing");
        trace!("calendars: {}", collections.len());
        Ok(collections)
    }

    /// Enumerates a calendar's lone events and series masters, in full.
    pub(super) fn enumerate_calendar(&mut self, calendar: &str) -> Result<Enumeration> {
        debug!("begin graph events listing");
        trace!("calendar: {calendar}");

        let params = MsgraphEventsListParams {
            top: Some(PAGE_SIZE),
            select: Some(LIST_SELECT),
            ..Default::default()
        };
        let mut page = self
            .op(|graph| graph.events_list(Some(calendar), &params))
            .with_context(|| format!("List events of {calendar} error"))?;

        let mut items = Vec::new();
        loop {
            for event in page.value {
                if event.id.is_empty() || event.series_master_id.is_some() {
                    continue;
                }
                items.push(EnumEntry {
                    id: event.id,
                    flags: Default::default(),
                    revision: event.change_key,
                });
            }

            let Some(next) = page.next_link else {
                break;
            };
            page = self
                .op(|graph| graph.events_list_from_link(&next))
                .with_context(|| format!("Page events of {calendar} error"))?;
        }

        debug!("end of graph events listing");
        trace!("events: {}", items.len());
        Ok(Enumeration {
            items,
            vanished: Vec::new(),
            complete: true,
            checkpoint: Vec::new(),
        })
    }

    /// Reads one event with everything the projection reads, its stash
    /// expanded.
    fn event(&mut self, id: &str) -> Result<MsgraphEvent> {
        self.op(|graph| {
            graph.event_get(
                id,
                Some(MSGRAPH_EVENT_ICAL_SELECT),
                Some(MSGRAPH_EVENT_STASH_EXPAND),
            )
        })
        .with_context(|| format!("Get event {id} error"))
    }

    /// Reads an item: the event, and the exceptions of a series.
    fn series(&mut self, id: &str) -> Result<(MsgraphEvent, Vec<MsgraphEvent>)> {
        let master = self.event(id)?;
        if master.event_type != Some(MsgraphEventType::SeriesMaster) {
            return Ok((master, Vec::new()));
        }

        let Some((start, end)) = series_window(&master) else {
            warn!("series {id} has no readable range, its exceptions are skipped");
            return Ok((master, Vec::new()));
        };

        let params = MsgraphEventsListParams {
            top: Some(PAGE_SIZE),
            // NOTE: an exception needs its originalStart for its
            // RECURRENCE-ID, which the default listing leaves out.
            select: Some(MSGRAPH_EVENT_ICAL_SELECT),
            ..Default::default()
        };
        let mut page = self
            .op(|graph| graph.event_instances(id, &start, &end, &params))
            .with_context(|| format!("List instances of {id} error"))?;

        let mut exceptions = Vec::new();
        loop {
            exceptions.extend(
                page.value
                    .into_iter()
                    .filter(|event| event.event_type == Some(MsgraphEventType::Exception)),
            );

            let Some(next) = page.next_link else {
                break;
            };
            page = self
                .op(|graph| graph.events_list_from_link(&next))
                .with_context(|| format!("Page instances of {id} error"))?;
        }

        Ok((master, exceptions))
    }

    /// Reads one item as iCalendar with the revision it corresponds to.
    fn calendar_item(&mut self, id: &str) -> Result<(String, Option<String>)> {
        let (master, exceptions) = self.series(id)?;
        let exceptions: Vec<&MsgraphEvent> = exceptions.iter().collect();
        Ok((master.to_ical_series(&exceptions), master.change_key))
    }

    /// Streams the iCalendar objects of an id set, each committed with its
    /// master's `changeKey`.
    pub(super) fn fetch_events<S: Write>(
        &mut self,
        ids: &[&str],
        mut open: impl FnMut(&str) -> std::io::Result<S>,
        mut done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<()> {
        for id in ids {
            let (ical, revision) = self.calendar_item(id)?;
            let mut sink = open(id).with_context(|| format!("Open body sink for {id} error"))?;
            sink.write_all(ical.as_bytes())
                .with_context(|| format!("Store event {id} error"))?;
            done(id, revision.as_deref(), sink)
                .with_context(|| format!("Commit event {id} error"))?;
        }
        Ok(())
    }

    /// Streams one item's iCalendar object into `sink`, returning its
    /// revision.
    pub(super) fn get_event_stream(
        &mut self,
        id: &str,
        mut sink: impl Write,
    ) -> Result<Option<String>> {
        let (ical, revision) = self.calendar_item(id)?;
        sink.write_all(ical.as_bytes())
            .with_context(|| format!("Stream event {id} error"))?;
        Ok(revision)
    }

    /// Creates an event from an iCalendar object in a calendar.
    ///
    /// The event is read back with its stash: one whose UID did not survive
    /// would read back under Graph's own `iCalUId`, a second event for every
    /// other source, so it is deleted again and the write refused.
    pub(super) fn add_event(&mut self, calendar: &str, source: impl Read) -> Result<WrittenItem> {
        let ical = read_bytes(source)?;
        let event = MsgraphEvent::create_from_ical(&ical)?;
        let uid = event.stashed_uid();

        let created = self
            .op(|graph| graph.event_create(Some(calendar), &event))
            .with_context(|| format!("Create event in {calendar} error"))?;
        let stored = self.event(&created.id)?;

        if uid.is_some() && stored.stashed_uid() != uid {
            if let Err(err) = self.op(|graph| graph.event_delete(&created.id)) {
                warn!(
                    "cannot delete event {} that lost its UID: {err}",
                    created.id
                );
            }
            bail!(
                "Graph did not keep the UID of the event created in {calendar}, \
                 so it would read back as another event"
            );
        }

        Ok(WrittenItem {
            id: stored.id,
            revision: stored.change_key,
        })
    }

    /// Replaces a series master from an iCalendar object, sending only what
    /// changed against the server copy, which also serves the `If-Match`
    /// check.
    pub(super) fn update_event(
        &mut self,
        id: &str,
        source: impl Read,
        if_match: Option<&str>,
    ) -> Result<Option<String>> {
        let ical = read_bytes(source)?;
        let (current, exceptions) = self.series(id)?;
        check_revision(id, &current, if_match)?;

        let exceptions: Vec<&MsgraphEvent> = exceptions.iter().collect();
        let base = current.to_ical_series(&exceptions);
        let patch = MsgraphEvent::update_from_ical(&ical, base.as_bytes())?;

        let updated = self
            .op(|graph| graph.event_update(id, &patch))
            .with_context(|| format!("Update event {id} error"))?;

        Ok(updated.change_key)
    }

    /// Deletes an event, the whole series for a master, conditionally on
    /// `if_match`.
    pub(super) fn delete_event(&mut self, id: &str, if_match: Option<&str>) -> Result<()> {
        if if_match.is_some() {
            let current = self.event(id)?;
            check_revision(id, &current, if_match)?;
        }

        self.op(|graph| graph.event_delete(id))
            .with_context(|| format!("Delete event {id} error"))?;
        Ok(())
    }
}

/// The window a series' exceptions fall in: its range, an open-ended one
/// capped past its start.
fn series_window(master: &MsgraphEvent) -> Option<(String, String)> {
    let range = &master.recurrence.as_option()?.range;
    let start = range.start_date.as_deref()?.parse::<Date>().ok()?;
    let end = match range.end_date.as_deref() {
        Some(end) => end.parse::<Date>().ok()?,
        None => start.checked_add(OPEN_SERIES_YEARS.years()).ok()?,
    };

    let instant = |date: Date| -> Option<String> {
        let timestamp: Timestamp = date.to_zoned(TimeZone::UTC).ok()?.timestamp();
        Some(timestamp.strftime("%Y-%m-%dT%H:%M:%SZ").to_string())
    };

    Some((instant(start)?, instant(end.checked_add(1.day()).ok()?)?))
}

/// Refuses a write when the event moved on Graph since `if_match`.
fn check_revision(id: &str, current: &MsgraphEvent, if_match: Option<&str>) -> Result<()> {
    match if_match {
        Some(expected) if current.change_key.as_deref() != Some(expected) => {
            bail!("Event {id} changed on Graph since the last sync, the next run picks the edit up")
        }
        _ => Ok(()),
    }
}

/// Reads a whole body.
fn read_bytes(mut source: impl Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    source
        .read_to_end(&mut bytes)
        .context("Read the iCalendar object to push to Graph error")?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use io_msgraph::v1::{
        field::MsgraphField,
        rest::users::events::{MsgraphPatternedRecurrence, MsgraphRecurrenceRange},
    };

    use super::*;

    #[test]
    fn a_series_window_spans_its_range_or_years_past_an_open_start() {
        let series = |end: Option<&str>| MsgraphEvent {
            recurrence: MsgraphField::Set(MsgraphPatternedRecurrence {
                range: MsgraphRecurrenceRange {
                    start_date: Some("2026-08-14".into()),
                    end_date: end.map(str::to_owned),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        };

        assert_eq!(
            series_window(&series(Some("2026-09-30"))),
            Some(("2026-08-14T00:00:00Z".into(), "2026-10-01T00:00:00Z".into()))
        );
        assert_eq!(
            series_window(&series(None)).map(|(_, end)| end),
            Some("2031-08-15T00:00:00Z".into())
        );
    }

    #[test]
    fn a_write_against_a_moved_event_is_refused() {
        let current = MsgraphEvent {
            change_key: Some("v2".into()),
            ..Default::default()
        };

        assert!(check_revision("e1", &current, Some("v2")).is_ok());
        assert!(check_revision("e1", &current, Some("v1")).is_err());
    }
}
