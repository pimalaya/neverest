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
//!
//! A new event the account organises and whose attendees are to be told
//! is inserted with its UID as `iCalUID`, which Google keeps and announces;
//! any other is imported, which keeps the UID and notifies nobody. The
//! invitation intents patch the account's own attendee, or delete the
//! meeting it organises, both with `sendUpdates=all`.

use std::{
    collections::BTreeMap,
    io::{Read, Write},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use io_gcal::v3::{
    client::{GcalClientStd, GcalClientStdConnectOptions, GcalClientStdError},
    rest::{
        calendar_list::list::GcalCalendarListListParams,
        events::{
            GcalEvent, GcalEventAttendeeResponseStatus, GcalEventDateTime, GcalEventStatus,
            GcalSendUpdates, import::GcalEventImportParams, insert::GcalEventInsertParams,
            instances::GcalEventInstancesParams, list::GcalEventsListParams,
            patch::GcalEventPatchParams, update::GcalEventUpdateParams,
        },
    },
    send::{GCAL_API_BASE, GcalSendError, GcalSendOutput},
};
use io_pimdir::summary::calendar;
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
    offline::invitation::{Partstat, Refusal, occurrence_window, utc_stamp, wall_stamp},
    throttle::{Request, Throttle, google_throttled_for},
};

/// The page size requested from the events listing.
const PAGE_SIZE: u32 = 2500;

/// The conference data version every event write declares: `1` honours a
/// create request and keeps the conference the merged payload carries.
const CONFERENCE_DATA_VERSION: Option<u8> = Some(1);

/// Whether the written event asks for a new Google Meet
/// (`X-PIMDIR-ONLINE-MEETING`, pimdir STORAGE Annex B.1).
///
/// Its write then reports no revision: the next sync reads the event back,
/// the meeting's `CONFERENCE` in and the request property out.
fn asks_meeting(event: &GcalEvent) -> bool {
    event
        .conference_data
        .as_ref()
        .is_some_and(|conference| conference.create_request.is_some())
}

/// The live Google Calendar session of one side.
pub struct GcalClient {
    inner: GcalClientStd,
    /// The TLS configuration, kept for stream reopens.
    tls: Tls,
    /// Whether the server allowed reusing the stream after the last
    /// exchange; when false the next operation reopens it.
    alive: bool,
    /// The source's back-off, shared by its connections.
    throttle: Arc<Throttle>,
    /// The account's own address, its primary calendar's id, read once
    /// when a new event first needs it.
    owner: Option<String>,
}

impl GcalClient {
    /// Opens the TLS connection to the Calendar API with a bearer token.
    pub fn connect(token: &SecretString, tls: Tls, throttle: Arc<Throttle>) -> Result<Self> {
        let options = GcalClientStdConnectOptions { tls: tls.clone() };
        let inner = GcalClientStd::connect(token.expose_secret(), options)
            .context("Cannot connect to Google Calendar")?;

        Ok(Self {
            inner,
            tls,
            alive: true,
            throttle,
            owner: None,
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
    ///
    /// The source's [`Throttle`] sends it again while Google throttles it
    /// (429, 503, or a 403 rate limit). A request creating an event goes
    /// through [`create`](Self::create) instead.
    fn op<T>(
        &mut self,
        run: impl FnMut(&mut GcalClientStd) -> Result<GcalSendOutput<T>, GcalClientStdError>,
    ) -> Result<T, GcalClientStdError> {
        self.send(Request::Idempotent, run)
    }

    /// Runs one Calendar request creating an event, which may invite its
    /// attendees: sent again on a refusal Google answers before doing
    /// anything (429, a rate limit), never on a 503.
    fn create<T>(
        &mut self,
        run: impl FnMut(&mut GcalClientStd) -> Result<GcalSendOutput<T>, GcalClientStdError>,
    ) -> Result<T, GcalClientStdError> {
        self.send(Request::Create, run)
    }

    /// Runs one Calendar request of `request`, as [`op`](Self::op) describes.
    fn send<T>(
        &mut self,
        request: Request,
        mut run: impl FnMut(&mut GcalClientStd) -> Result<GcalSendOutput<T>, GcalClientStdError>,
    ) -> Result<T, GcalClientStdError> {
        let throttle = Arc::clone(&self.throttle);
        throttle.call(
            1,
            || {
                if !self.alive
                    && let Err(err) = self.reconnect()
                {
                    warn!("cannot reopen the calendar stream: {err:#}");
                }

                let out = run(&mut self.inner)?;
                self.alive = out.keep_alive;
                Ok(out.response)
            },
            |err| match err {
                GcalClientStdError::Send(GcalSendError::Api { status, message }) => {
                    google_throttled_for(request, *status, message)
                }
                _ => None,
            },
            |message| {
                GcalClientStdError::Send(GcalSendError::Api {
                    status: 429,
                    message,
                })
            },
        )
    }

    /// Lists the calendars of the user's calendar list, keyed by id and
    /// named by the user's own name for them.
    pub fn list_collections(&mut self) -> Result<Vec<Collection>> {
        Ok(self.calendars()?.0)
    }

    /// The id of the user's primary calendar, the one the calendar list
    /// marks `primary`. A check-only probe.
    pub fn default_collection(&mut self) -> Result<Option<String>> {
        Ok(self.calendars()?.1)
    }

    /// Lists the calendars, and names the primary one.
    fn calendars(&mut self) -> Result<(Vec<Collection>, Option<String>)> {
        let mut collections = Vec::new();
        let mut primary = None;
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
                let is_primary = primary.is_none() && entry.primary == Some(true);
                if is_primary {
                    primary = Some(id.clone());
                }
                let name = entry
                    .summary_override
                    .or(entry.summary)
                    .unwrap_or_else(|| id.clone());
                collections.push(Collection {
                    id,
                    name,
                    total: None,
                    unread: None,
                    role: is_primary.then(|| String::from("default")),
                });
            }

            match page.next_page_token {
                Some(next) => page_token = Some(next),
                None => return Ok((collections, primary)),
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
            .map(|(id, events)| EnumEntry::bare(id, Default::default(), Some(revision(&events))))
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
            checkpoint: Some(next.into_bytes()),
            ..Default::default()
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
    /// One carrying no UID is inserted, Google minting it. One carrying a
    /// UID keeps it as the event's `iCalUID`, so it matches its copy on
    /// another source: inserted with it when the resource is scheduled and
    /// the account organises it, Google then inviting the attendees, and
    /// imported otherwise, which notifies nobody (pimdir STORAGE Annex B.1).
    pub fn add_item_stream(&mut self, calendar: &str, source: impl Read) -> Result<WrittenItem> {
        let ical = read_ical(source)?;
        let mut event = GcalEvent::from_ical(&ical)?;
        let meeting = asks_meeting(&event);

        let has_uid = event.ical_uid.as_deref().is_some_and(|uid| !uid.is_empty());
        let invites = has_uid && calendar::scheduled(&ical) && self.organises(calendar, &event)?;
        let created = if invites {
            // NOTE: on an insert the organizer is Google's to set: the
            // calendar the event lands on.
            event.organizer = None;
            let params = GcalEventInsertParams {
                send_updates: Some(GcalSendUpdates::All),
                conference_data_version: CONFERENCE_DATA_VERSION,
                ..Default::default()
            };
            self.create(|gcal| gcal.event_insert(calendar, &event, &params))
                .with_context(|| format!("Insert scheduled event into {calendar} error"))?
        } else if has_uid {
            let params = GcalEventImportParams {
                conference_data_version: CONFERENCE_DATA_VERSION,
                ..Default::default()
            };
            self.create(|gcal| gcal.event_import(calendar, &event, &params))
                .with_context(|| format!("Import event into {calendar} error"))?
        } else {
            let params = GcalEventInsertParams {
                conference_data_version: CONFERENCE_DATA_VERSION,
                ..Default::default()
            };
            self.create(|gcal| gcal.event_insert(calendar, &event, &params))
                .with_context(|| format!("Insert event into {calendar} error"))?
        };

        let revision = revision(std::slice::from_ref(&created));
        let id = created
            .id
            .context("Google returned the created event without an id")?;

        Ok(WrittenItem {
            id,
            revision: (!meeting).then_some(revision),
        })
    }

    /// Whether the account organises a new event: it names no organizer,
    /// or the calendar it lands on, or the account's own address.
    fn organises(&mut self, calendar: &str, event: &GcalEvent) -> Result<bool> {
        let Some(organizer) = event
            .organizer
            .as_ref()
            .and_then(|organizer| organizer.email.as_deref())
            .filter(|email| !email.is_empty())
        else {
            return Ok(true);
        };
        if organizer.eq_ignore_ascii_case(calendar) {
            return Ok(true);
        }

        let owner = match &self.owner {
            Some(owner) => owner.clone(),
            None => {
                let primary = self
                    .op(|gcal| gcal.calendar_list_entry_get("primary"))
                    .context("Get the primary Google calendar error")?;
                let owner = primary.id.unwrap_or_default();
                self.owner = Some(owner.clone());
                owner
            }
        };
        Ok(organizer.eq_ignore_ascii_case(&owner))
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
        let meeting = asks_meeting(&projected);

        let mut events = self.series(calendar, id)?;
        check_revision(id, &events, if_match)?;
        let current = events.remove(0);

        // NOTE: the attendees are notified unless the resource marks them
        // for the client or for nobody (pimdir STORAGE Annex B.1).
        let send_updates = match calendar::scheduled(&ical) {
            true => GcalSendUpdates::All,
            false => GcalSendUpdates::None,
        };
        let event = projected.merge(&current);
        let updated = self
            .op(|gcal| {
                gcal.event_update(
                    calendar,
                    id,
                    &event,
                    &GcalEventUpdateParams {
                        send_updates: Some(send_updates),
                        conference_data_version: CONFERENCE_DATA_VERSION,
                        ..Default::default()
                    },
                    current.etag.as_deref(),
                )
            })
            .with_context(|| format!("Update event {id} of {calendar} error"))?;

        if meeting {
            return Ok(None);
        }

        events.insert(0, updated);
        Ok(Some(revision(&events)))
    }

    /// Deletes a series, refused when it moved since `if_match`.
    pub fn delete_item(&mut self, calendar: &str, id: &str, if_match: Option<&str>) -> Result<()> {
        let events = self.series(calendar, id)?;
        check_revision(id, &events, if_match)?;
        let etag = events.first().and_then(|event| event.etag.clone());
        let send_updates = match events
            .first()
            .is_some_and(|event| !event.attendees.is_empty())
        {
            true => GcalSendUpdates::All,
            false => GcalSendUpdates::None,
        };

        self.op(|gcal| gcal.event_delete(calendar, id, Some(send_updates), etag.as_deref()))
            .with_context(|| format!("Delete event {id} of {calendar} error"))?;
        Ok(())
    }

    /// The id of the instance of series `id` that a `RECURRENCE-ID` value
    /// names, compared the way the projection writes it (the wall time of
    /// its original start, a date, or a UTC stamp), among the instances
    /// Google lists two days either side of its date. None found refuses.
    pub fn occurrence(&mut self, calendar: &str, id: &str, recurrence_id: &str) -> Result<String> {
        let Some((start, end)) = occurrence_window(recurrence_id) else {
            return Err(anyhow::Error::new(Refusal(format!(
                "{recurrence_id} names no occurrence"
            ))));
        };

        let mut page_token: Option<String> = None;
        loop {
            let params = GcalEventInstancesParams {
                time_min: Some(&start),
                time_max: Some(&end),
                page_token: page_token.as_deref(),
                ..Default::default()
            };
            let page = self
                .op(|gcal| gcal.event_instances(calendar, id, &params))
                .with_context(|| format!("List instances of {id} error"))?;

            let found = page.items.into_iter().find(|instance| {
                instance
                    .original_start_time
                    .as_ref()
                    .is_some_and(|original| names_occurrence(original, recurrence_id))
            });
            if let Some(instance_id) = found.and_then(|instance| instance.id) {
                return Ok(instance_id);
            }

            match page.next_page_token {
                Some(next) => page_token = Some(next),
                None => break,
            }
        }

        Err(anyhow::Error::new(Refusal(format!(
            "Series {id} has no occurrence {recurrence_id}"
        ))))
    }

    /// Answers an invitation as the calendar's own attendee, the organizer
    /// told with `comment` (pimdir STORAGE Annex B.2 `calendar.reply`).
    ///
    /// Only that attendee changes, on the attendee list read right before
    /// and guarded by its etag.
    pub fn reply(
        &mut self,
        calendar: &str,
        id: &str,
        partstat: Partstat,
        comment: Option<&str>,
    ) -> Result<()> {
        let current = self
            .op(|gcal| gcal.event_get(calendar, id, None, None))
            .with_context(|| format!("Get event {id} of {calendar} error"))?;
        let patch = reply_patch(&current, partstat, comment)?;
        let params = GcalEventPatchParams {
            send_updates: Some(GcalSendUpdates::All),
            ..Default::default()
        };

        self.op(|gcal| gcal.event_patch(calendar, id, &patch, &params, current.etag.as_deref()))
            .with_context(|| format!("Reply to event {id} of {calendar} error"))?;
        Ok(())
    }

    /// Cancels a meeting the account organises, Google telling its
    /// attendees (pimdir STORAGE Annex B.2 `calendar.cancel`). Google sends
    /// its own notice, which carries no comment.
    pub fn cancel(&mut self, calendar: &str, id: &str) -> Result<()> {
        let current = self
            .op(|gcal| gcal.event_get(calendar, id, None, None))
            .with_context(|| format!("Get event {id} of {calendar} error"))?;
        if is_cancelled(&current) {
            return Ok(());
        }
        let organises = current
            .organizer
            .as_ref()
            .is_some_and(|organizer| organizer.is_self == Some(true));
        if !organises {
            return Err(anyhow::Error::new(Refusal(format!(
                "The account does not organise event {id}, so it cannot cancel it"
            ))));
        }

        self.op(|gcal| {
            gcal.event_delete(
                calendar,
                id,
                Some(GcalSendUpdates::All),
                current.etag.as_deref(),
            )
        })
        .with_context(|| format!("Cancel event {id} of {calendar} error"))?;
        Ok(())
    }

    /// Rejected: calendar events have no flags.
    pub fn store_flags(&mut self, _ids: &[&str], _flags: &[Flag], _op: FlagOp) -> Result<()> {
        bail!("Google calendar events have no flags (store not supported)")
    }
}

/// The patch answering an invitation: the attendee list with the account's
/// own entry answered, refused when the account is no attendee or
/// organises the event.
fn reply_patch(
    current: &GcalEvent,
    partstat: Partstat,
    comment: Option<&str>,
) -> Result<GcalEvent> {
    let mut attendees = current.attendees.clone();
    let id = current.id.as_deref().unwrap_or_default();
    let Some(me) = attendees
        .iter_mut()
        .find(|attendee| attendee.is_self == Some(true))
    else {
        return Err(anyhow::Error::new(Refusal(format!(
            "The account is not an attendee of event {id}, so it cannot reply"
        ))));
    };
    if me.organizer == Some(true) {
        return Err(anyhow::Error::new(Refusal(format!(
            "The account organises event {id}, so it has no invitation to reply to"
        ))));
    }

    me.response_status = Some(match partstat {
        Partstat::Accepted => GcalEventAttendeeResponseStatus::Accepted,
        Partstat::Tentative => GcalEventAttendeeResponseStatus::Tentative,
        Partstat::Declined => GcalEventAttendeeResponseStatus::Declined,
    });
    if let Some(comment) = comment {
        me.comment = Some(comment.to_owned());
    }

    Ok(GcalEvent {
        attendees,
        ..Default::default()
    })
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

/// Whether an instance's original start is the occurrence a
/// `RECURRENCE-ID` value names: its date, its wall time, or its UTC stamp.
fn names_occurrence(original: &GcalEventDateTime, recurrence_id: &str) -> bool {
    let stamps = [
        original.date.as_deref().and_then(wall_stamp),
        original.date_time.as_deref().and_then(wall_stamp),
        original.date_time.as_deref().and_then(utc_stamp),
    ];
    stamps.iter().flatten().any(|stamp| stamp == recurrence_id)
}

#[cfg(test)]
mod tests {
    use io_gcal::v3::rest::events::GcalEventAttendee;

    use super::*;

    #[test]
    fn an_instance_is_the_occurrence_its_original_start_names() {
        let timed = GcalEventDateTime {
            date_time: Some("2026-10-06T09:00:00+02:00".into()),
            time_zone: Some("Europe/Paris".into()),
            ..Default::default()
        };
        assert!(names_occurrence(&timed, "20261006T090000"));
        assert!(names_occurrence(&timed, "20261006T070000Z"));
        assert!(!names_occurrence(&timed, "20261006T080000"));
        assert!(!names_occurrence(&timed, "20261013T090000"));

        let all_day = GcalEventDateTime {
            date: Some("2026-10-06".into()),
            ..Default::default()
        };
        assert!(names_occurrence(&all_day, "20261006"));
        assert!(!names_occurrence(&all_day, "20261007"));
    }

    fn attendee(email: &str, me: bool, organizer: bool) -> GcalEventAttendee {
        GcalEventAttendee {
            email: Some(email.to_owned()),
            is_self: me.then_some(true),
            organizer: organizer.then_some(true),
            response_status: Some(GcalEventAttendeeResponseStatus::NeedsAction),
            ..Default::default()
        }
    }

    #[test]
    fn a_reply_answers_the_account_alone() {
        let current = GcalEvent {
            id: Some("meeting".into()),
            attendees: vec![
                attendee("alice@example.org", false, true),
                attendee("me@example.org", true, false),
                attendee("bob@example.org", false, false),
            ],
            ..Default::default()
        };

        let patch = reply_patch(&current, Partstat::Declined, Some("Away")).unwrap();

        assert_eq!(patch.attendees.len(), 3);
        assert_eq!(patch.attendees[0], current.attendees[0]);
        assert_eq!(patch.attendees[2], current.attendees[2]);
        assert_eq!(
            patch.attendees[1].response_status,
            Some(GcalEventAttendeeResponseStatus::Declined)
        );
        assert_eq!(patch.attendees[1].comment.as_deref(), Some("Away"));
        assert!(patch.summary.is_none() && patch.start.is_none());
    }

    #[test]
    fn a_reply_is_refused_to_the_organizer_and_to_a_stranger() {
        let organised = GcalEvent {
            attendees: vec![attendee("me@example.org", true, true)],
            ..Default::default()
        };
        let foreign = GcalEvent {
            attendees: vec![attendee("alice@example.org", false, true)],
            ..Default::default()
        };

        assert!(reply_patch(&organised, Partstat::Accepted, None).is_err());
        assert!(reply_patch(&foreign, Partstat::Accepted, None).is_err());
    }

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
