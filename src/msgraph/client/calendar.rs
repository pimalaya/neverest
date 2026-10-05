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
//!
//! The invitation intents go through the event's own actions, `accept`,
//! `tentativelyAccept`, `decline` and `cancel`, Graph sending the iTIP
//! message with the comment.

use std::{
    collections::BTreeMap,
    io::{Read, Write},
};

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
    send::{MSGRAPH_API_BASE, MsgraphNoResponse, MsgraphSend, user_path},
};
use jiff::{Timestamp, ToSpan, civil::Date, tz::TimeZone};
use log::{debug, trace, warn};
use serde_json::json;
use url::Url;

use super::GraphClient;
use crate::{
    client::{EnumEntry, Enumeration, WrittenItem},
    item::collection::Collection,
    offline::invitation::{Partstat, Refusal, occurrence_window, utc_stamp},
};

/// The `$select` of the enumeration: what tells an event's identity, its
/// kind and its revision.
const LIST_SELECT: &str = "id,changeKey,type,seriesMasterId";

/// The page size requested when listing events.
const PAGE_SIZE: u32 = 500;

/// How far either side of today an open-ended series is searched for
/// exceptions.
const OPEN_SERIES_YEARS: i16 = 5;

/// The longest window Graph lists a series' instances over.
const MAX_WINDOW_YEARS: i16 = 5;

impl GraphClient {
    /// Lists the user's calendars, keyed by id and named by their name.
    pub(super) fn list_calendars(&mut self) -> Result<Vec<Collection>> {
        Ok(self.calendars()?.0)
    }

    /// The id of the user's default calendar (`isDefaultCalendar`), the
    /// one an invitation lands in.
    pub(super) fn default_calendar(&mut self) -> Result<Option<String>> {
        Ok(self.calendars()?.1)
    }

    /// Lists the user's calendars, and names the default one.
    fn calendars(&mut self) -> Result<(Vec<Collection>, Option<String>)> {
        debug!("begin graph calendar listing");

        let params = MsgraphCalendarsListParams {
            top: Some(100),
            ..Default::default()
        };
        let mut page = self
            .op(|graph| graph.calendars_list(&params))
            .context("List calendars error")?;

        let mut collections = Vec::new();
        let mut default = None;
        loop {
            for calendar in page.value {
                if calendar.id.is_empty() {
                    continue;
                }
                let is_default = default.is_none() && calendar.is_default_calendar == Some(true);
                if is_default {
                    default = Some(calendar.id.clone());
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
                    role: is_default.then(|| String::from("default")),
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
        Ok((collections, default))
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

        let mut events = Vec::new();
        loop {
            events.extend(page.value);

            let Some(next) = page.next_link else {
                break;
            };
            page = self
                .op(|graph| graph.events_list_from_link(&next))
                .with_context(|| format!("Page events of {calendar} error"))?;
        }

        let items: Vec<EnumEntry> = unique(events)?
            .into_iter()
            .filter(|event| !event.id.is_empty() && event.series_master_id.is_none())
            .map(|event| EnumEntry {
                id: event.id,
                flags: Default::default(),
                revision: event.change_key,
            })
            .collect();

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

        let today = Timestamp::now().to_zoned(TimeZone::UTC).date();
        let Some(windows) = series_windows(&master, today) else {
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

        let mut exceptions = Vec::new();
        for (start, end) in windows {
            let mut page = self
                .op(|graph| graph.event_instances(id, &start, &end, &params))
                .with_context(|| format!("List instances of {id} error"))?;

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
        }

        Ok((master, unique(exceptions)?))
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

    /// The id of the instance of series `id` that a `RECURRENCE-ID` value
    /// names, compared the way the projection writes it (the wall time in
    /// the series' zone, a date, or a UTC stamp), among the instances Graph
    /// lists two days either side of its date. None found refuses.
    pub fn occurrence_event(&mut self, id: &str, recurrence_id: &str) -> Result<String> {
        let master = self.event(id)?;
        if master.event_type != Some(MsgraphEventType::SeriesMaster) {
            return Err(anyhow::Error::new(Refusal(format!(
                "Event {id} is not a series, it has no occurrence {recurrence_id}"
            ))));
        }
        let Some((start, end)) = occurrence_window(recurrence_id) else {
            return Err(anyhow::Error::new(Refusal(format!(
                "{recurrence_id} names no occurrence"
            ))));
        };

        let params = MsgraphEventsListParams {
            top: Some(PAGE_SIZE),
            select: Some(MSGRAPH_EVENT_ICAL_SELECT),
            ..Default::default()
        };
        let mut page = self
            .op(|graph| graph.event_instances(id, &start, &end, &params))
            .with_context(|| format!("List instances of {id} error"))?;

        loop {
            let found = page.value.into_iter().find(|instance| {
                instance.original_start.as_deref().is_some_and(|original| {
                    master.recurrence_id_of(original).as_deref() == Some(recurrence_id)
                        || utc_stamp(original).as_deref() == Some(recurrence_id)
                })
            });
            if let Some(instance) = found {
                return Ok(instance.id);
            }

            let Some(next) = page.next_link else {
                break;
            };
            page = self
                .op(|graph| graph.events_list_from_link(&next))
                .with_context(|| format!("Page instances of {id} error"))?;
        }

        Err(anyhow::Error::new(Refusal(format!(
            "Series {id} has no occurrence {recurrence_id}"
        ))))
    }

    /// Answers an invitation, Graph sending the reply to the organizer with
    /// `comment` (pimdir STORAGE Annex B.2 `calendar.reply`).
    ///
    /// Graph refuses it (400) on an event the account organises.
    pub fn reply_event(
        &mut self,
        id: &str,
        partstat: Partstat,
        comment: Option<&str>,
    ) -> Result<()> {
        let verb = match partstat {
            Partstat::Accepted => "accept",
            Partstat::Tentative => "tentativelyAccept",
            Partstat::Declined => "decline",
        };
        let body = json!({
            "comment": comment.unwrap_or_default(),
            "sendResponse": true,
        });

        self.event_action(id, verb, &body)
            .with_context(|| format!("Reply to event {id} error"))
    }

    /// Cancels a meeting the account organises, Graph sending the
    /// cancellation to the attendees with `comment` (pimdir STORAGE Annex
    /// B.2 `calendar.cancel`).
    ///
    /// Graph refuses it (400) on an event the account does not organise.
    pub fn cancel_event(&mut self, id: &str, comment: Option<&str>) -> Result<()> {
        let body = json!({ "comment": comment.unwrap_or_default() });

        self.event_action(id, "cancel", &body)
            .with_context(|| format!("Cancel event {id} error"))
    }

    /// Posts one of an event's actions, which answer 202 with no body.
    fn event_action(&mut self, id: &str, verb: &str, body: &serde_json::Value) -> Result<()> {
        let path = format!("{}/events/{id}/{verb}", user_path(&self.inner.user_id));
        let url = Url::parse(MSGRAPH_API_BASE)
            .and_then(|base| base.join(&path))
            .context("Cannot build the event action URL")?;

        self.op(|graph| {
            let coroutine = MsgraphSend::<MsgraphNoResponse>::post_json(&graph.auth, url, body)?;
            graph.run(coroutine)
        })?;
        Ok(())
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

/// Drops the events a page boundary repeated, Graph overlapping
/// consecutive `nextLink` pages; one id read at two revisions is an error.
fn unique(events: Vec<MsgraphEvent>) -> Result<Vec<MsgraphEvent>> {
    let mut seen = BTreeMap::new();
    let mut unique = Vec::with_capacity(events.len());

    for event in events {
        match seen.get(&event.id) {
            None => {
                seen.insert(event.id.clone(), event.change_key.clone());
                unique.push(event);
            }
            Some(change_key) if *change_key == event.change_key => {}
            Some(_) => bail!(
                "Event {} listed twice at different revisions by Graph",
                event.id
            ),
        }
    }

    Ok(unique)
}

/// The windows a series' exceptions are read in, none longer than the five
/// years Graph allows an instances listing (it answers 400 past them).
///
/// A bounded series is read over its whole range. An open-ended one, as
/// Birthdays are, is read within [`OPEN_SERIES_YEARS`] either side of
/// `today`: one born in 1604, Outlook's year for a birthday without one,
/// would otherwise take dozens of requests, and an exception that far back
/// changes no occurrence anybody looks at.
fn series_windows(master: &MsgraphEvent, today: Date) -> Option<Vec<(String, String)>> {
    let (start, end) = master.recurrence.as_option()?.bounds()?;
    let (start, end) = match end {
        Some(end) => (start, end.checked_add(1.day()).ok()?),
        None => {
            let years = OPEN_SERIES_YEARS.years();
            let floor = today.checked_sub(years).ok()?;
            (start.max(floor), today.checked_add(years).ok()?)
        }
    };

    let instant = |date: Date| -> Option<String> {
        let timestamp: Timestamp = date.to_zoned(TimeZone::UTC).ok()?.timestamp();
        Some(timestamp.strftime("%Y-%m-%dT%H:%M:%SZ").to_string())
    };

    let mut windows = Vec::new();
    let mut from = start;
    while from < end {
        let longest = from
            .checked_add(MAX_WINDOW_YEARS.years())
            .ok()?
            .checked_sub(1.day())
            .ok()?;
        let to = longest.min(end);
        windows.push((instant(from)?, instant(to)?));
        from = to;
    }

    Some(windows)
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
        rest::users::events::{
            MsgraphPatternedRecurrence, MsgraphRecurrenceRange, MsgraphRecurrenceRangeType,
        },
    };

    use super::*;

    fn event(id: &str, change_key: &str) -> MsgraphEvent {
        MsgraphEvent {
            id: id.into(),
            change_key: Some(change_key.into()),
            ..Default::default()
        }
    }

    #[test]
    fn series_windows_span_the_range_within_what_graph_allows() {
        let series = |range_type, start: &str, end: &str| MsgraphEvent {
            recurrence: MsgraphField::Set(MsgraphPatternedRecurrence {
                range: MsgraphRecurrenceRange {
                    range_type: Some(range_type),
                    start_date: Some(start.into()),
                    end_date: Some(end.into()),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        };
        let today: Date = "2026-10-04".parse().unwrap();
        let windows = |range_type, start, end| {
            series_windows(&series(range_type, start, end), today).unwrap()
        };
        let pair = |from: &str, to: &str| (format!("{from}T00:00:00Z"), format!("{to}T00:00:00Z"));

        assert_eq!(
            windows(
                MsgraphRecurrenceRangeType::EndDate,
                "2026-08-14",
                "2026-09-30"
            ),
            [pair("2026-08-14", "2026-10-01")]
        );
        // NOTE: twelve years, read in windows Graph accepts.
        assert_eq!(
            windows(
                MsgraphRecurrenceRangeType::EndDate,
                "2020-01-01",
                "2031-12-31"
            ),
            [
                pair("2020-01-01", "2024-12-31"),
                pair("2024-12-31", "2029-12-30"),
                pair("2029-12-30", "2032-01-01"),
            ]
        );
        // NOTE: Graph fills the endDate of a noEnd range with a sentinel;
        // a birthday from 1604 is read around today, not since then.
        assert_eq!(
            windows(
                MsgraphRecurrenceRangeType::NoEnd,
                "1604-03-12",
                "0001-01-01"
            ),
            [
                pair("2021-10-04", "2026-10-03"),
                pair("2026-10-03", "2031-10-02"),
                pair("2031-10-02", "2031-10-04"),
            ]
        );
        assert_eq!(
            windows(
                MsgraphRecurrenceRangeType::NoEnd,
                "2026-08-14",
                "0001-01-01"
            ),
            [
                pair("2026-08-14", "2031-08-13"),
                pair("2031-08-13", "2031-10-04")
            ]
        );
    }

    #[test]
    fn an_event_repeated_across_pages_lists_once() {
        let events = vec![event("A", "1"), event("B", "1"), event("B", "1")];
        let ids: Vec<String> = unique(events)
            .unwrap()
            .into_iter()
            .map(|event| event.id)
            .collect();

        assert_eq!(ids, ["A", "B"]);
    }

    #[test]
    fn an_event_repeated_at_another_revision_is_refused() {
        assert!(unique(vec![event("A", "1"), event("A", "2")]).is_err());
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
