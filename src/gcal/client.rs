//! # Google Calendar client
//!
//! [`GcalClient`] wraps io-gcal's std client behind the adapter surface the
//! sync engine needs, projecting events onto iCalendar through io-gcal's
//! `ical` feature.
//!
//! The calendars of the user's calendar list are the collections. One item
//! is one recurring series with its modified instances, or one lone event,
//! keyed by the series' event id, the way a CalDAV resource holds a whole
//! series: Google returns a modified instance as an event of its own,
//! pointing at its series through `recurringEventId`.
//!
//! `enumerate` drives the events listing with a sync token as the engine's
//! opaque checkpoint; an expired token (HTTP 410) restarts a full listing.
//! An item's revision joins the etags of the series and of its instances,
//! so an edit to any one of them moves it. A resumed listing only sees the
//! events that changed, so its revision for a series is partial; it still
//! differs from the stored one, and the body read records the whole one.
//!
//! An update and a delete are checked against that joined revision, then
//! guarded natively by the series event's own etag. Only the series event
//! is written: an instance modified locally does not push yet.

use std::{
    collections::BTreeMap,
    io::{Read, Write},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use io_gcal::v3::{
    client::{GcalClientStd, GcalClientStdConnectOptions, GcalClientStdError},
    rest::{
        calendar_list::list::GcalCalendarListListParams,
        events::{
            GcalEvent, GcalEventStatus, import::GcalEventImportParams,
            insert::GcalEventInsertParams, list::GcalEventsListParams,
            update::GcalEventUpdateParams,
        },
    },
    send::{GCAL_API_BASE, GcalSendOutput},
};
use log::{debug, trace, warn};
use pimalaya_stream::{
    stream::{Stream, TlsConnectOptions},
    tls::Tls,
};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::{
    client::{EnumEntry, Enumeration, WrittenItem},
    item::{
        collection::Collection,
        flag::{Flag, FlagOp},
    },
};

/// The page size requested from the events listing.
const PAGE_SIZE: u32 = 2500;

/// The live Google Calendar session of one side.
pub struct GcalClient {
    inner: GcalClientStd,
    /// The TLS configuration, kept for stream reopens.
    tls: Tls,
    /// Whether the server allowed reusing the stream after the last
    /// exchange; when false the next operation reopens it.
    alive: bool,
}

impl GcalClient {
    /// Opens the TLS connection to the Calendar API with a bearer token.
    pub fn connect(token: &SecretString, tls: Tls) -> Result<Self> {
        let options = GcalClientStdConnectOptions { tls: tls.clone() };
        let inner = GcalClientStd::connect(token.expose_secret(), options)
            .context("Cannot connect to Google Calendar")?;

        Ok(Self {
            inner,
            tls,
            alive: true,
        })
    }

    /// Reopens the stream to the Calendar endpoint, keeping the credential.
    fn reconnect(&mut self) -> Result<()> {
        debug!("reopening the calendar stream");

        let url = Url::parse(GCAL_API_BASE).context("Cannot parse the Calendar API base URL")?;
        let host = url
            .host_str()
            .context("Calendar API base URL has no host")?;
        let opts = TlsConnectOptions {
            tls: self.tls.clone(),
            ..Default::default()
        };
        let stream = Stream::connect_tls(host, url.port().unwrap_or(443), opts)?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;

        self.inner.set_stream(stream);
        self.alive = true;
        Ok(())
    }

    /// Runs one Calendar operation, reopening the stream first when the
    /// server closed it, and records the new keep-alive hint.
    fn op<T>(
        &mut self,
        run: impl FnOnce(&mut GcalClientStd) -> Result<GcalSendOutput<T>, GcalClientStdError>,
    ) -> Result<T, GcalClientStdError> {
        if !self.alive
            && let Err(err) = self.reconnect()
        {
            warn!("cannot reopen the calendar stream: {err:#}");
        }

        let out = run(&mut self.inner)?;
        self.alive = out.keep_alive;
        Ok(out.response)
    }

    /// Lists the calendars of the user's calendar list, keyed by id and
    /// named by the user's own name for them.
    pub fn list_collections(&mut self) -> Result<Vec<Collection>> {
        let mut collections = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let params = GcalCalendarListListParams {
                page_token: page_token.as_deref(),
                ..Default::default()
            };
            let page = self
                .op(|gcal| gcal.calendar_list_list(&params))
                .context("List Google calendars error")?;

            for entry in page.items {
                let Some(id) = entry.id.filter(|id| !id.is_empty()) else {
                    continue;
                };
                let name = entry
                    .summary_override
                    .or(entry.summary)
                    .unwrap_or_else(|| id.clone());
                collections.push(Collection {
                    id,
                    name,
                    total: None,
                    unread: None,
                });
            }

            match page.next_page_token {
                Some(next) => page_token = Some(next),
                None => return Ok(collections),
            }
        }
    }

    /// Enumerates a calendar, incrementally from the checkpoint's sync
    /// token when it holds one and the token has not expired.
    pub fn enumerate(&mut self, calendar: &str, cursor: Option<&[u8]>) -> Result<Enumeration> {
        let token = cursor.and_then(|bytes| String::from_utf8(bytes.to_vec()).ok());
        let (events, complete, next) = match token {
            None => self.list(calendar, None)?,
            Some(token) => match self.list(calendar, Some(&token)) {
                Err(err) if is_expired(&err) => {
                    warn!("calendar sync token of {calendar} expired, restarting a full listing");
                    self.list(calendar, None)?
                }
                result => result?,
            },
        };

        let mut series: BTreeMap<String, Vec<GcalEvent>> = BTreeMap::new();
        let mut vanished = Vec::new();
        for event in events {
            let Some(id) = event.id.clone().filter(|id| !id.is_empty()) else {
                continue;
            };
            match event.recurring_event_id.clone() {
                Some(master) => series.entry(master).or_default().push(event),
                None if is_cancelled(&event) => vanished.push(id),
                None => series.entry(id).or_default().insert(0, event),
            }
        }

        let items = series
            .into_iter()
            .filter(|(id, _)| !vanished.contains(id))
            .map(|(id, events)| EnumEntry {
                id,
                flags: Default::default(),
                revision: Some(revision(&events)),
            })
            .collect();

        // NOTE: a full listing reports removals by absence; only a resumed
        // one names them.
        if complete {
            vanished.clear();
        }

        Ok(Enumeration {
            items,
            vanished,
            complete,
            checkpoint: next.into_bytes(),
        })
    }

    /// Pages through a calendar's events, series and their modified
    /// instances rather than expanded occurrences, deleted ones included,
    /// returning the events, whether the listing was full, and the next
    /// sync token.
    fn list(
        &mut self,
        calendar: &str,
        token: Option<&str>,
    ) -> Result<(Vec<GcalEvent>, bool, String)> {
        debug!("begin calendar events listing");
        trace!("calendar: {calendar}, resumed: {}", token.is_some());

        let mut events = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let params = GcalEventsListParams {
                max_results: Some(PAGE_SIZE),
                page_token: page_token.as_deref(),
                show_deleted: true,
                sync_token: token,
                ..Default::default()
            };
            let page = self
                .op(|gcal| gcal.events_list(calendar, &params))
                .map_err(anyhow::Error::new)
                .with_context(|| format!("List events of {calendar} error"))?;

            events.extend(page.items);

            match page.next_page_token {
                Some(next) => page_token = Some(next),
                None => {
                    let next = page.next_sync_token.with_context(|| {
                        format!("The listing of {calendar} returned no sync token")
                    })?;
                    debug!("end of calendar events listing");
                    trace!("events: {}", events.len());
                    return Ok((events, token.is_none(), next));
                }
            }
        }
    }

    /// Reads a series event with every modified instance of it, the series
    /// first.
    fn series(&mut self, calendar: &str, id: &str) -> Result<Vec<GcalEvent>> {
        let master = self
            .op(|gcal| gcal.event_get(calendar, id, None, None))
            .with_context(|| format!("Get event {id} of {calendar} error"))?;

        let mut events = vec![master.clone()];
        let Some(uid) = master
            .ical_uid
            .clone()
            .filter(|_| !master.recurrence.is_empty())
        else {
            return Ok(events);
        };

        let mut page_token: Option<String> = None;
        loop {
            let params = GcalEventsListParams {
                ical_uid: Some(&uid),
                page_token: page_token.as_deref(),
                show_deleted: true,
                ..Default::default()
            };
            let page = self
                .op(|gcal| gcal.events_list(calendar, &params))
                .with_context(|| format!("List instances of {id} error"))?;

            events.extend(
                page.items
                    .into_iter()
                    .filter(|event| event.recurring_event_id.as_deref() == Some(id)),
            );

            match page.next_page_token {
                Some(next) => page_token = Some(next),
                None => return Ok(events),
            }
        }
    }

    /// Reads one item as iCalendar with the revision it corresponds to.
    fn item(&mut self, calendar: &str, id: &str) -> Result<(String, String)> {
        let events = self.series(calendar, id)?;
        let (master, instances) = events.split_first().context("Empty event series")?;
        let instances: Vec<&GcalEvent> = instances.iter().collect();

        Ok((master.to_ical_series(&instances), revision(&events)))
    }

    /// Streams the iCalendar objects of an id set, each committed with its
    /// joined revision.
    pub fn fetch_bodies<S: Write>(
        &mut self,
        calendar: &str,
        ids: &[&str],
        mut open: impl FnMut(&str) -> std::io::Result<S>,
        mut done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<()> {
        for id in ids {
            let (ical, revision) = self.item(calendar, id)?;
            let mut sink = open(id).with_context(|| format!("Open body sink for {id} error"))?;
            sink.write_all(ical.as_bytes())
                .with_context(|| format!("Store event {id} error"))?;
            done(id, Some(&revision), sink).with_context(|| format!("Commit event {id} error"))?;
        }
        Ok(())
    }

    /// Streams one item's iCalendar object into `sink`, returning its
    /// joined revision.
    pub fn get_item_stream(
        &mut self,
        calendar: &str,
        id: &str,
        mut sink: impl Write,
    ) -> Result<Option<String>> {
        let (ical, revision) = self.item(calendar, id)?;
        sink.write_all(ical.as_bytes())
            .with_context(|| format!("Stream event {id} error"))?;
        Ok(Some(revision))
    }

    /// Creates an event from an iCalendar object.
    ///
    /// One carrying a UID is imported, so the UID survives as the event's
    /// `iCalUID` and matches its copy on another source; one carrying none
    /// is inserted, Google minting it.
    pub fn add_item_stream(&mut self, calendar: &str, source: impl Read) -> Result<WrittenItem> {
        let ical = read_ical(source)?;
        let event = GcalEvent::from_ical(&ical)?;

        let created = match event.ical_uid.as_deref().filter(|uid| !uid.is_empty()) {
            Some(_) => self
                .op(|gcal| gcal.event_import(calendar, &event, &GcalEventImportParams::default()))
                .with_context(|| format!("Import event into {calendar} error"))?,
            None => self
                .op(|gcal| gcal.event_insert(calendar, &event, &GcalEventInsertParams::default()))
                .with_context(|| format!("Insert event into {calendar} error"))?,
        };

        let revision = revision(std::slice::from_ref(&created));
        let id = created
            .id
            .context("Google returned the created event without an id")?;

        Ok(WrittenItem {
            id,
            revision: Some(revision),
        })
    }

    /// Replaces a series event from an iCalendar object, merged onto the
    /// server copy so the fields iCalendar does not model survive.
    pub fn update_item_stream(
        &mut self,
        calendar: &str,
        id: &str,
        source: impl Read,
        if_match: Option<&str>,
    ) -> Result<Option<String>> {
        let ical = read_ical(source)?;
        let projected = GcalEvent::from_ical(&ical)?;

        let mut events = self.series(calendar, id)?;
        check_revision(id, &events, if_match)?;
        let current = events.remove(0);

        let event = projected.merge(&current);
        let updated = self
            .op(|gcal| {
                gcal.event_update(
                    calendar,
                    id,
                    &event,
                    &GcalEventUpdateParams::default(),
                    current.etag.as_deref(),
                )
            })
            .with_context(|| format!("Update event {id} of {calendar} error"))?;

        events.insert(0, updated);
        Ok(Some(revision(&events)))
    }

    /// Deletes a series, refused when it moved since `if_match`.
    pub fn delete_item(&mut self, calendar: &str, id: &str, if_match: Option<&str>) -> Result<()> {
        let events = self.series(calendar, id)?;
        check_revision(id, &events, if_match)?;
        let etag = events.first().and_then(|event| event.etag.clone());

        self.op(|gcal| gcal.event_delete(calendar, id, None, etag.as_deref()))
            .with_context(|| format!("Delete event {id} of {calendar} error"))?;
        Ok(())
    }

    /// Rejected: calendar events have no flags.
    pub fn store_flags(&mut self, _ids: &[&str], _flags: &[Flag], _op: FlagOp) -> Result<()> {
        bail!("Google calendar events have no flags (store not supported)")
    }
}

/// The joined revision of a series: its events' etags, the series first
/// and the instances by id.
fn revision(events: &[GcalEvent]) -> String {
    let mut etags: Vec<(&str, &str)> = events
        .iter()
        .map(|event| {
            let order = match event.recurring_event_id {
                Some(_) => event.id.as_deref().unwrap_or_default(),
                None => "",
            };
            (order, event.etag.as_deref().unwrap_or_default())
        })
        .collect();
    etags.sort();
    etags
        .into_iter()
        .map(|(_, etag)| etag)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether an event was cancelled, Google's tombstone for a deletion.
fn is_cancelled(event: &GcalEvent) -> bool {
    event.status == Some(GcalEventStatus::Cancelled)
}

/// Refuses a write when the series moved on Google since `if_match`.
fn check_revision(id: &str, events: &[GcalEvent], if_match: Option<&str>) -> Result<()> {
    match if_match {
        Some(expected) if revision(events) != expected => bail!(
            "Event {id} changed on Google since the last sync, the next run picks the edit up"
        ),
        _ => Ok(()),
    }
}

/// Whether a listing failed on an expired sync token (HTTP 410).
fn is_expired(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<GcalClientStdError>(),
            Some(GcalClientStdError::Send(send)) if send.status() == Some(410)
        )
    })
}

/// Reads a whole iCalendar body as raw bytes.
fn read_ical(mut source: impl Read) -> Result<Vec<u8>> {
    let mut ical = Vec::new();
    source
        .read_to_end(&mut ical)
        .context("Read the iCalendar object to push to Google error")?;
    Ok(ical)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, master: Option<&str>, etag: &str) -> GcalEvent {
        GcalEvent {
            id: Some(id.to_owned()),
            recurring_event_id: master.map(str::to_owned),
            etag: Some(etag.to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn a_series_revision_moves_with_any_of_its_events() {
        let series = [
            event("standup", None, "\"1\""),
            event("standup_b", Some("standup"), "\"3\""),
            event("standup_a", Some("standup"), "\"2\""),
        ];
        let edited = [
            event("standup", None, "\"1\""),
            event("standup_b", Some("standup"), "\"4\""),
            event("standup_a", Some("standup"), "\"2\""),
        ];

        assert_eq!(revision(&series), "\"1\" \"2\" \"3\"");
        assert_ne!(revision(&series), revision(&edited));
        assert!(check_revision("standup", &series, Some("\"1\" \"2\" \"3\"")).is_ok());
        assert!(check_revision("standup", &edited, Some("\"1\" \"2\" \"3\"")).is_err());
    }

    #[test]
    fn a_lone_event_revision_is_its_etag() {
        assert_eq!(revision(&[event("lunch", None, "\"7\"")]), "\"7\"");
    }
}
