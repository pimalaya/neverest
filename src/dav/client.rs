//! # DAV client
//!
//! [`DavClient`] wraps the std blocking io-webdav client behind the same
//! adapter surface as the IMAP and Graph backends, serving CardDAV (RFC 6352)
//! and CalDAV (RFC 4791) alike.
//!
//! One adapter serves both because they differ only in the home set they
//! discover, the collection they list and the extension a new resource is
//! named with ([`DavKind`]). This is the mutable-content side of the sync,
//! where the ETag and `If-Match` plumbing built for mail is finally exercised.
//!
//! Three shapes differ from the mail backends: collections are keyed by their
//! path segment under the home set, else by their path, starting with a slash
//! (io-webdav's id), a display name being optional, mutable and free to
//! collide;
//! items resolve at `Full` only, a `sync-collection` REPORT carrying hrefs and
//! ETags but never a `UID`; flags are known-empty rather than unknown.
//!
//! Enumeration is RFC 6578 where the server implements it: a rejected token
//! ([`WebdavSyncCollectionError::InvalidSyncToken`]) is answered with a fresh
//! full report, and a truncated one (§3.6) is drained by running it again from
//! the token it returned.
//!
//! That report is an extension, though, and a server implementing none of it
//! refuses with the RFC 3253 §3.6 `DAV:supported-report` precondition. Such a
//! collection is listed with a `PROPFIND` instead, which yields the same ids
//! and ETags with no token, rather than not syncing at all.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    io::{ErrorKind, Read, Write},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use io_http::rfc9112::send::Http11SendError;
use io_webdav::{
    client::{WebdavClientStd, WebdavClientStdConnectOptions, WebdavClientStdError},
    coroutine::{WebdavCoroutine, WebdavCoroutineState, WebdavYield},
    rfc4791::calendar::{CALDAV, CaldavCalendar},
    rfc4918::{
        WebdavAuth, WebdavProperty, collection_id, follow_redirects::WebdavFollowRedirectsError,
        propfind::WebdavPropfind, resolve_href, send::WebdavSendError,
    },
    rfc6352::addressbook::CarddavAddressbook,
    rfc6578::sync_collection::{
        SYNC_COLLECTION, WebdavSyncChange, WebdavSyncCollectionError, WebdavSyncCollectionOptions,
        WebdavSyncDelta,
    },
};
use log::{debug, warn};
use pimalaya_stream::{proxy::Proxy, tls::Tls};
use url::Url;

use crate::{
    client::{EnumEntry, Enumeration, WrittenItem},
    item::{collection::Collection, flag::Flag, flag::FlagOp},
    kind::{Kind, LinkId},
    throttle::{Request, Throttle},
};

/// How many truncated rounds one enumeration drains before giving up, so a
/// server answering "truncated" forever cannot spin the sync.
const MAX_SYNC_ROUNDS: usize = 32;

/// Which DAV flavour a session speaks: the whole of the protocol difference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DavKind {
    /// CardDAV (RFC 6352): address books of contact cards.
    Card,
    /// CalDAV (RFC 4791): calendars of calendar object resources.
    Cal,
}

impl DavKind {
    /// The IANA media type of the resources this flavour syncs, recorded as the
    /// pimdir collection's `kind`.
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Card => "text/vcard",
            Self::Cal => "text/calendar",
        }
    }

    /// The item kind whose derivations read those resources.
    fn item_kind(self) -> Kind {
        match self {
            Self::Card => Kind::Vcard,
            Self::Cal => Kind::Ical,
        }
    }

    /// The configuration table this flavour is written under, also the id of
    /// the source the direct-backend sugar builds from it.
    #[cfg(feature = "wizard")]
    pub fn protocol(self) -> &'static str {
        match self {
            Self::Card => "carddav",
            Self::Cal => "caldav",
        }
    }

    /// The conventional extension a new resource is named with.
    fn extension(self) -> &'static str {
        match self {
            Self::Card => "vcf",
            Self::Cal => "ics",
        }
    }
}

impl fmt::Display for DavKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Card => write!(f, "CardDAV"),
            Self::Cal => write!(f, "CalDAV"),
        }
    }
}

/// A live DAV session, scoped to one home set.
pub struct DavClient {
    kind: DavKind,
    inner: WebdavClientStd,
    /// The connect arguments, kept so a closed connection can be reopened.
    server: Url,
    tls: Tls,
    auth: WebdavAuth,
    /// The source's back-off, shared by its connections.
    throttle: Arc<Throttle>,
}

impl DavClient {
    /// Opens the session and discovers the home set, so a misconfigured URL or
    /// credential fails here rather than at the first enumeration.
    pub fn connect(
        kind: DavKind,
        server: &Url,
        tls: &Tls,
        auth: WebdavAuth,
        throttle: Arc<Throttle>,
    ) -> Result<Self> {
        let inner = WebdavClientStd::connect(server, auth.clone(), connect_options(tls))
            .with_context(|| format!("Cannot connect to the {kind} server"))?;
        let mut client = Self {
            kind,
            inner,
            server: server.clone(),
            tls: tls.clone(),
            auth,
            throttle,
        };

        let home = match kind {
            DavKind::Card => client.op(WebdavClientStd::addressbook_home_set),
            DavKind::Cal => client.op(WebdavClientStd::calendar_home_set),
        }
        .with_context(|| format!("Cannot discover the {kind} home set"))?;
        debug!("{kind} home set: {home}");

        Ok(client)
    }

    /// The media type of the items this session syncs.
    pub fn media_type(&self) -> &'static str {
        self.kind.media_type()
    }

    /// Runs one WebDAV exchange, reopening a connection the server had closed.
    ///
    /// io-webdav holds a single stream, so an HTTP/1.0 or `Connection: close`
    /// peer breaks the exchange after an answer; a client from `connect`
    /// resends it once on a fresh connection itself, and this reopens one
    /// should that fail too. Only an end-of-stream failure retries, never an
    /// applied write.
    ///
    /// The source's [`Throttle`] also sends it again while the server
    /// throttles it (429, 503). A request creating a resource goes through
    /// [`op_create`](Self::op_create) instead.
    fn op<T>(
        &mut self,
        run: impl FnMut(&mut WebdavClientStd) -> Result<T, WebdavClientStdError>,
    ) -> Result<T, WebdavClientStdError> {
        self.send(Request::Idempotent, run)
    }

    /// Runs one WebDAV exchange creating a resource or a collection: sent
    /// again on a 429 alone, which a server answers before applying
    /// anything, never on a 503, after which the resource may exist.
    fn op_create<T>(
        &mut self,
        run: impl FnMut(&mut WebdavClientStd) -> Result<T, WebdavClientStdError>,
    ) -> Result<T, WebdavClientStdError> {
        self.send(Request::Create, run)
    }

    /// Runs one WebDAV exchange of `request`, as [`op`](Self::op) describes.
    fn send<T>(
        &mut self,
        request: Request,
        mut run: impl FnMut(&mut WebdavClientStd) -> Result<T, WebdavClientStdError>,
    ) -> Result<T, WebdavClientStdError> {
        let throttle = Arc::clone(&self.throttle);
        throttle.call(
            1,
            || match run(&mut self.inner) {
                Err(err) if is_connection_closed(&err) => {
                    debug!("{} connection closed by the server, reopening", self.kind);
                    self.reconnect()?;
                    run(&mut self.inner)
                }
                out => out,
            },
            |err| {
                throttle_status(err)
                    .is_some_and(|status| request.retries(status))
                    .then_some(None)
            },
            |body| WebdavClientStdError::Send(WebdavSendError::HttpStatus { status: 429, body }),
        )
    }

    /// Reopens the connection, carrying over the discovery already paid for.
    ///
    /// Both home sets are copied without asking which one this session
    /// discovered: only its own is ever populated, so the other copies an
    /// absence.
    fn reconnect(&mut self) -> Result<(), WebdavClientStdError> {
        let mut inner =
            WebdavClientStd::connect(&self.server, self.auth.clone(), connect_options(&self.tls))?;
        inner.principal_url = self.inner.principal_url.clone();
        inner.addressbook_home_set = self.inner.addressbook_home_set.clone();
        inner.addressbook_reports = self.inner.addressbook_reports.clone();
        inner.calendar_home_set = self.inner.calendar_home_set.clone();
        inner.calendar_reports = self.inner.calendar_reports.clone();
        self.inner = inner;

        Ok(())
    }

    /// Lists every collection of this session's kind.
    ///
    /// Counts are never reported: DAV has no cheap total, and paying a full
    /// enumeration per collection to render one number is not a trade this
    /// makes.
    pub fn list_collections(&mut self, _with_counts: bool) -> Result<Vec<Collection>> {
        let kind = self.kind;
        // NOTE: by id, the display name and whether the server states the
        // collection takes tasks.
        let collections: BTreeMap<String, (Option<String>, Option<bool>)> = match kind {
            DavKind::Card => self.op(WebdavClientStd::list_addressbooks).map(|books| {
                books
                    .into_iter()
                    .map(|book| (book.id, (book.display_name, None)))
                    .collect()
            }),
            DavKind::Cal => self.op(WebdavClientStd::list_calendars).map(|calendars| {
                calendars
                    .into_iter()
                    .map(|calendar| {
                        let tasks = takes_tasks(&calendar.components);
                        (calendar.id, (calendar.display_name, tasks))
                    })
                    .collect()
            }),
        }
        .with_context(|| format!("Cannot list the {kind} collections"))?;

        // NOTE: the id is the key (the path segment, or the path of one
        // outside the home set), a display name being free to collide, to
        // change and to be absent; it is still what the collection is called,
        // and servers routinely make the segment a UUID.
        Ok(collections
            .into_iter()
            .map(|(id, (display, takes_tasks))| Collection {
                name: display
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| id.clone()),
                id,
                total: None,
                unread: None,
                role: None,
                takes_tasks,
            })
            .collect())
    }

    /// The collection a new item goes to when none is named, by id.
    ///
    /// For CalDAV, the calendar the scheduling inbox names as
    /// `schedule-default-calendar-URL` (RFC 6638 §9.2), when the server
    /// gives it; a server without scheduling answers none. CardDAV names
    /// no default address book. A check-only probe: a failed lookup is no
    /// default, never an error.
    pub fn default_collection(&mut self) -> Result<Option<String>> {
        if self.kind == DavKind::Card {
            return Ok(None);
        }

        match self.schedule_default_calendar() {
            Ok(id) => Ok(id),
            Err(err) => {
                debug!("no default calendar stated: {err}");
                Ok(None)
            }
        }
    }

    /// Follows the principal's `schedule-inbox-URL` to the inbox's
    /// `schedule-default-calendar-URL`, and keys it as a listing does.
    fn schedule_default_calendar(&mut self) -> Result<Option<String>, WebdavClientStdError> {
        let principal = self.op(WebdavClientStd::current_user_principal)?;
        let Some(inbox) = self.propfind_href(principal.path(), SCHEDULE_INBOX_URL)? else {
            return Ok(None);
        };
        let Some(calendar) = self.propfind_href(inbox.path(), SCHEDULE_DEFAULT_CALENDAR_URL)?
        else {
            return Ok(None);
        };
        let Some(home) = &self.inner.calendar_home_set else {
            return Ok(None);
        };
        Ok(collection_key(home.path(), calendar.path()))
    }

    /// The href a `Depth: 0` PROPFIND of `path` states for `prop`.
    fn propfind_href(
        &mut self,
        path: &str,
        prop: WebdavProperty,
    ) -> Result<Option<Url>, WebdavClientStdError> {
        let multistatus = self.op(|dav| {
            let coroutine =
                WebdavPropfind::new(&dav.base_url, dav.auth(), &dav.user_agent, path, 0, &[prop]);
            run(dav, coroutine)
        })?;
        let base = self.inner.base_url.clone();
        Ok(multistatus
            .responses
            .iter()
            .find_map(|entry| entry.text(prop))
            .and_then(|href| resolve_href(&base, href)))
    }

    /// Creates a collection, named after the collection key.
    pub fn create_collection(&mut self, collection: &str) -> Result<()> {
        self.create_collection_named(collection, collection)
    }

    /// Creates the collection keyed `collection` (its path segment) and
    /// displayed as `name`.
    pub fn create_collection_named(&mut self, collection: &str, name: &str) -> Result<()> {
        let kind = self.kind;
        let name = Some(name.to_owned());

        match kind {
            DavKind::Card => {
                let book = CarddavAddressbook {
                    id: collection.to_owned(),
                    display_name: name,
                    ..Default::default()
                };
                self.op_create(|dav| dav.create_addressbook(&book))
            }
            DavKind::Cal => {
                let calendar = CaldavCalendar {
                    id: collection.to_owned(),
                    display_name: name,
                    ..Default::default()
                };
                self.op_create(|dav| dav.create_calendar(&calendar))
            }
        }
        .with_context(|| format!("Cannot create the {kind} collection {collection}"))
    }

    /// Deletes a collection.
    pub fn delete_collection(&mut self, collection: &str) -> Result<()> {
        let kind = self.kind;

        match kind {
            DavKind::Card => self.op(|dav| dav.delete_addressbook(collection)),
            DavKind::Cal => self.op(|dav| dav.delete_calendar(collection)),
        }
        .with_context(|| format!("Cannot delete the {kind} collection {collection}"))
    }

    /// Enumerates through `sync-collection`, its token the engine's checkpoint.
    ///
    /// RFC 6578 is an extension, so a server may refuse the REPORT with the RFC
    /// 3253 §3.6 `DAV:supported-report` precondition; the collection is listed
    /// instead, which without the fallback would be unsyncable.
    pub fn enumerate(&mut self, collection: &str, cursor: Option<&[u8]>) -> Result<Enumeration> {
        if !has_sync_collection(self.reports(collection)) {
            debug!(
                "{} server advertises no `sync-collection` for {collection}, listing it",
                self.kind
            );
            return self
                .list(collection)
                .with_context(|| format!("Cannot list {collection}"));
        }

        // NOTE: an empty checkpoint is the listing fallback's, meaning no
        // cursor.
        let token = cursor
            .filter(|cursor| !cursor.is_empty())
            .map(String::from_utf8_lossy)
            .map(String::from);

        match self.sync(collection, token.as_deref()) {
            Ok(enumeration) => Ok(enumeration),
            Err(err) if is_invalid_sync_token(&err) => {
                warn!(
                    "{} sync token rejected for {collection}, enumerating in full",
                    self.kind
                );
                self.sync(collection, None)
                    .with_context(|| format!("Cannot enumerate {collection} in full"))
            }
            Err(err) if err.is_unsupported_report() => {
                debug!(
                    "{} server has no `sync-collection`, listing {collection} instead",
                    self.kind
                );
                self.list(collection)
                    .with_context(|| format!("Cannot list {collection}"))
            }
            Err(err) => {
                Err(anyhow::Error::new(err).context(format!("Cannot enumerate {collection}")))
            }
        }
    }

    /// The reports the server advertises for a collection, read from the
    /// `supported-report-set` io-webdav caches while listing.
    fn reports(&self, collection: &str) -> Option<&BTreeSet<String>> {
        match self.kind {
            DavKind::Card => self.inner.addressbook_reports.get(collection),
            DavKind::Cal => self.inner.calendar_reports.get(collection),
        }
    }

    /// One `sync-collection` REPORT, or the `PROPFIND` listing standing in for
    /// it under [`WebdavSyncCollectionOptions::fallback`].
    fn sync_collection(
        &mut self,
        collection: &str,
        token: Option<&str>,
        opts: WebdavSyncCollectionOptions,
    ) -> Result<WebdavSyncDelta, WebdavClientStdError> {
        match self.kind {
            DavKind::Card => self.op(|dav| dav.sync_cards(collection, token, opts)),
            DavKind::Cal => self.op(|dav| dav.sync_items(collection, token, opts)),
        }
    }

    /// One full `PROPFIND` listing, for a server with no `sync-collection`.
    ///
    /// It carries no token, so every run lists the whole collection. A
    /// `PROPFIND` rather than the query report: a query filter is evaluated by
    /// parsing every resource, so one unparsable resource fails the listing.
    fn list(&mut self, collection: &str) -> Result<Enumeration, WebdavClientStdError> {
        let opts = WebdavSyncCollectionOptions { fallback: true };
        let delta = self.sync_collection(collection, None, opts)?;

        if delta.truncated {
            warn!(
                "{} server truncated the listing of {collection}, reconciling as a delta",
                self.kind
            );
        }

        Ok(Enumeration {
            items: delta.changed.into_iter().map(entry).collect(),
            vanished: Vec::new(),
            // NOTE: a partial snapshot read as complete deletes members left
            // out.
            complete: !delta.truncated,
            checkpoint: Some(Vec::new()),
            ..Default::default()
        })
    }

    /// One enumeration, draining every truncated round into a single result.
    fn sync(
        &mut self,
        collection: &str,
        token: Option<&str>,
    ) -> Result<Enumeration, WebdavClientStdError> {
        let complete = token.is_none();
        let mut items = Vec::new();
        let mut vanished = Vec::new();
        let mut token = token.map(String::from);

        for round in 0..MAX_SYNC_ROUNDS {
            let delta = self.sync_collection(collection, token.as_deref(), Default::default())?;

            items.extend(delta.changed.into_iter().map(entry));
            vanished.extend(delta.vanished.iter().map(|href| href_id(href)));
            token = delta.sync_token;

            if !delta.truncated {
                break;
            }
            if round + 1 == MAX_SYNC_ROUNDS {
                warn!(
                    "{} server kept truncating {collection} after {MAX_SYNC_ROUNDS} rounds, \
                     continuing with what it returned",
                    self.kind
                );
            }
        }

        Ok(Enumeration {
            items,
            vanished,
            complete,
            checkpoint: Some(token.unwrap_or_default().into_bytes()),
            ..Default::default()
        })
    }

    /// Batch-fetches bodies in one multiget round-trip, routing each into the
    /// sink the caller opens for it.
    pub fn fetch_bodies<S: Write>(
        &mut self,
        collection: &str,
        ids: &[&str],
        mut open: impl FnMut(&str) -> std::io::Result<S>,
        mut done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let kind = self.kind;
        let objects: Vec<(String, Option<String>, Vec<u8>)> = match kind {
            DavKind::Card => self
                .op(|dav| dav.multiget_cards(collection, ids))
                .map(|cards| {
                    cards
                        .into_iter()
                        .map(|card| (card.id, card.etag, card.data))
                        .collect()
                }),
            DavKind::Cal => self
                .op(|dav| dav.multiget_items(collection, ids))
                .map(|items| {
                    items
                        .into_iter()
                        .map(|item| (item.id, item.etag, item.data))
                        .collect()
                }),
        }
        .with_context(|| format!("Cannot fetch {kind} bodies from {collection}"))?;

        for (id, etag, data) in objects {
            let mut sink = open(&id)?;
            sink.write_all(&data)?;

            done(&id, etag.as_deref(), sink)?;
        }

        Ok(())
    }

    /// Streams one resource's raw bytes, returning the ETag they correspond to.
    pub fn get_item_stream(
        &mut self,
        collection: &str,
        id: &str,
        mut sink: impl Write,
    ) -> Result<Option<String>> {
        let (data, etag) = self
            .read(collection, id)
            .with_context(|| format!("Cannot read {id} in {collection}"))?;

        sink.write_all(&data)?;

        Ok(etag)
    }

    /// Creates a resource, addressed by the item's `UID`.
    ///
    /// The minted part of the key joins it where there is one, so a second copy
    /// of that `UID` never lands on the href its twin holds.
    pub fn add_item_stream(
        &mut self,
        collection: &str,
        mut source: impl Read,
        link: LinkId<'_>,
    ) -> Result<WrittenItem> {
        let mut body = Vec::new();
        source.read_to_end(&mut body)?;

        let id = resource_id(self.kind, link, &body);

        self.create(collection, &id, body)
            .with_context(|| format!("Cannot create {id} in {collection}"))
    }

    /// Replaces a resource in place, conditionally on the last-synced ETag.
    ///
    /// A server whose copy moved since rejects the write rather than losing the
    /// other edit, which is what the engine's conflict path waits for.
    pub fn update_item_stream(
        &mut self,
        collection: &str,
        id: &str,
        mut source: impl Read,
        if_match: Option<&str>,
    ) -> Result<Option<String>> {
        let mut body = Vec::new();
        source.read_to_end(&mut body)?;

        let updated = match self.kind {
            DavKind::Card => self
                .op(|dav| dav.update_card(collection, id, body.clone(), if_match))
                .map(|updated| updated.etag),
            DavKind::Cal => self
                .op(|dav| dav.update_item(collection, id, body.clone(), if_match))
                .map(|updated| updated.etag),
        };

        updated.with_context(|| format!("Cannot update {id} in {collection}"))
    }

    /// Deletes a resource, conditionally on the last-synced ETag.
    ///
    /// A resource already gone (404, 410) is deleted: pushes are at least
    /// once, and a create delivered by a move took the member it superseded
    /// along before that member's delete is pushed (pimdir SYNC §4).
    pub fn delete_item(
        &mut self,
        collection: &str,
        id: &str,
        if_match: Option<&str>,
    ) -> Result<()> {
        match self.delete(collection, id, if_match) {
            Err(err) if is_gone(&err) => {
                debug!("{id} is already gone from {collection}: {err}");
                Ok(())
            }
            deleted => deleted.with_context(|| format!("Cannot delete {id} from {collection}")),
        }
    }

    /// Moves resources between collections.
    ///
    /// Each goes by a server-side `MOVE` (see [`move_item`](Self::move_item)),
    /// which keeps its `UID` in one place: Fastmail, holding one `UID` per
    /// account, refuses a create at the target while the source holds it.
    /// Where the server has no `MOVE`, and for CardDAV, which io-webdav moves
    /// nothing of, the resource is re-created at the target and deleted from
    /// the source; the delete only runs once the create was accepted, so a
    /// failure leaves the resource where it was.
    pub fn move_items(&mut self, from: &str, to: &str, ids: &[&str]) -> Result<()> {
        for id in ids {
            if self.move_item(from, to, id, None)?.is_some() {
                continue;
            }

            let (data, etag) = self
                .read(from, id)
                .with_context(|| format!("Cannot read {id} for a move"))?;

            self.create(to, id, data)
                .with_context(|| format!("Cannot create {id} in {to}"))?;

            self.delete(from, id, etag.as_deref())
                .with_context(|| format!("Cannot delete the moved {id} from {from}"))?;
        }

        Ok(())
    }

    /// Moves one resource into `to` server side, keeping its id (WebDAV
    /// `MOVE`, RFC 4918 §9.9, `Overwrite: F`), gated on `if_match`.
    ///
    /// Answers the moved resource and the ETag the server gave it, if any, or
    /// `None` when this server cannot move it: CardDAV, which io-webdav moves
    /// nothing of, and a server answering the `MOVE` 405, 501 or 404, the last
    /// being Google's answer for a member it holds (pimdir SYNC §4). Any
    /// other refusal is an error, a duplicate `UID` among them
    /// ([`is_duplicate_uid`]).
    pub fn move_item(
        &mut self,
        from: &str,
        to: &str,
        id: &str,
        if_match: Option<&str>,
    ) -> Result<Option<WrittenItem>> {
        if self.kind == DavKind::Card {
            return Ok(None);
        }

        // NOTE: a create at the target, which a 503 may have applied.
        match self.op_create(|dav| dav.move_item(from, id, to, if_match)) {
            Ok(moved) => Ok(Some(WrittenItem {
                id: moved.id,
                revision: moved.etag,
            })),
            Err(err) if is_unsupported_move(&err) => {
                debug!("{} server cannot move {id} from {from}: {err}", self.kind);
                Ok(None)
            }
            Err(err) => {
                Err(anyhow::Error::new(err)
                    .context(format!("Cannot move {id} from {from} into {to}")))
            }
        }
    }

    /// Rejected: DAV has no flags. The enumeration reports every member as
    /// known-empty, and a flag kept in the store only is never pushed, the
    /// flag right being withheld for a kind other than mail.
    pub fn store_flags(&mut self, _ids: &[&str], _flags: &[Flag], _op: FlagOp) -> Result<()> {
        bail!("{} has no flags (store not supported)", self.kind)
    }

    /// Reads one resource's raw bytes and the ETag they correspond to.
    fn read(
        &mut self,
        collection: &str,
        id: &str,
    ) -> Result<(Vec<u8>, Option<String>), WebdavClientStdError> {
        match self.kind {
            DavKind::Card => self
                .op(|dav| dav.read_card(collection, id))
                .map(|card| (card.data, card.etag)),
            DavKind::Cal => self
                .op(|dav| dav.read_item(collection, id))
                .map(|item| (item.data, item.etag)),
        }
    }

    /// Creates one resource under the given name, reporting what the server
    /// assigned it.
    fn create(
        &mut self,
        collection: &str,
        id: &str,
        body: Vec<u8>,
    ) -> Result<WrittenItem, WebdavClientStdError> {
        match self.kind {
            DavKind::Card => self
                .op_create(|dav| dav.create_card(collection, id, body.clone()))
                .map(|created| WrittenItem {
                    id: created.id,
                    revision: created.etag,
                }),
            DavKind::Cal => self
                .op_create(|dav| dav.create_item(collection, id, body.clone()))
                .map(|created| WrittenItem {
                    id: created.id,
                    revision: created.etag,
                }),
        }
    }

    /// Deletes one resource, conditionally on an ETag.
    fn delete(
        &mut self,
        collection: &str,
        id: &str,
        if_match: Option<&str>,
    ) -> Result<(), WebdavClientStdError> {
        match self.kind {
            DavKind::Card => self.op(|dav| dav.delete_card(collection, id, if_match)),
            DavKind::Cal => self.op(|dav| dav.delete_item(collection, id, if_match)),
        }
    }
}

/// The scheduling inbox of a principal (RFC 6638 §2.2.1).
const SCHEDULE_INBOX_URL: WebdavProperty = WebdavProperty {
    ns: CALDAV,
    local: "schedule-inbox-URL",
};

/// The calendar a scheduling inbox files new invitations in (RFC 6638
/// §9.2).
const SCHEDULE_DEFAULT_CALENDAR_URL: WebdavProperty = WebdavProperty {
    ns: CALDAV,
    local: "schedule-default-calendar-URL",
};

/// The key a listing gives the collection at `path` under the home set
/// path `home`, as io-webdav keys a calendar: its last segment directly
/// under the home set, else its path, starting with a slash (Google lists
/// every calendar as `/caldav/v2/<calendar>/events/`).
fn collection_key(home: &str, path: &str) -> Option<String> {
    let key = collection_id(home, path);
    (!key.trim_matches('/').is_empty()).then_some(key)
}

/// Runs a coroutine io-webdav's client has no method for over its stream.
fn run<C, T, E>(dav: &mut WebdavClientStd, mut coroutine: C) -> Result<T, WebdavClientStdError>
where
    C: WebdavCoroutine<Yield = WebdavYield, Return = Result<T, E>>,
    E: Into<WebdavClientStdError>,
{
    let mut buf = [0u8; 8192];
    let mut read = None;

    loop {
        let arg = read.take().map(|n| &buf[..n]);
        match coroutine.resume(arg) {
            WebdavCoroutineState::Complete(out) => return out.map_err(Into::into),
            WebdavCoroutineState::Yielded(WebdavYield::WantsWrite(bytes)) => {
                dav.stream.write_all(&bytes)?;
            }
            WebdavCoroutineState::Yielded(WebdavYield::WantsRead) => {
                read = Some(dav.stream.read(&mut buf)?);
            }
        }
    }
}
/// The connect options of a session: the configured TLS, and a direct
/// connection, as every other source of the account opens its own.
fn connect_options(tls: &Tls) -> WebdavClientStdConnectOptions {
    WebdavClientStdConnectOptions {
        tls: tls.clone(),
        proxy: Proxy::None,
    }
}

/// One changed member of a `sync-collection` delta as an enumeration entry.
fn entry(change: WebdavSyncChange) -> EnumEntry {
    EnumEntry {
        id: href_id(&change.href),
        flags: BTreeSet::new(),
        revision: change.etag,
        meta: None,
    }
}

/// The addressing id of a member href: its last non-empty path segment,
/// as io-webdav addresses resources, in the one spelling of it
/// [`canonical_segment`] gives.
///
/// A server is free to spell a segment's octets literally or
/// percent-encoded: Stalwart lists `urn:uuid:x.vcf`, created under that
/// name, as `urn%3Auuid%3Ax.vcf`, which read as a second resource and
/// duplicated the item.
fn href_id(href: &str) -> String {
    let segment = href
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(href);
    canonical_segment(segment)
}

/// One spelling for every way of writing a segment neverest names: an
/// encoded unreserved octet decoded (RFC 3986 §6.2.2.2), a literal `:`
/// encoded, and every escape in upper case.
///
/// The colon goes encoded rather than literal: a relative reference whose
/// first segment carries one reads as a scheme (RFC 3986 §4.2), and
/// io-webdav reading `urn:uuid:x.vcf` back got a 404. Any other escape, a
/// `/` or a space, stays encoded, decoding it would name another resource.
fn canonical_segment(segment: &str) -> String {
    let mut canonical = String::with_capacity(segment.len());
    let mut rest = segment;
    while let Some(char) = rest.chars().next() {
        let escaped = rest
            .strip_prefix('%')
            .and_then(|tail| tail.get(..2))
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (char, escaped) {
            (_, Some(byte)) => {
                if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                    canonical.push(char::from(byte));
                } else {
                    canonical.push_str(&format!("%{byte:02X}"));
                }
                rest = &rest[3..];
            }
            (':', None) => {
                canonical.push_str("%3A");
                rest = &rest[1..];
            }
            (char, None) => {
                canonical.push(char);
                rest = &rest[char.len_utf8()..];
            }
        }
    }
    canonical
}

/// The resource name a new item is created under: `UID`, mint, extension.
///
/// The body fallback is only for an item stating no identity at all: a minted
/// key states one its twin already took, and a colliding `PUT` is not refused
/// but applied to the resource already there, losing it and reporting success.
fn resource_id(kind: DavKind, link: LinkId<'_>, body: &[u8]) -> String {
    let extension = kind.extension();
    let hint = link.hint.map(str::trim).filter(|hint| !hint.is_empty());

    let name = match (hint, link.mint) {
        (Some(uid), None) => sanitize(uid),
        (Some(uid), Some(mint)) => format!("{}-{}", sanitize(uid), sanitize(mint)),
        (None, Some(mint)) => sanitize(mint),
        (None, None) => {
            let link = kind.item_kind().parse_body(body, body.len() as u64).link_id;
            sanitize(link.0.trim_start_matches("hash:"))
        }
    };

    format!("{name}.{extension}")
}

/// Keeps a `UID` addressable as one path segment: a `UID` is free-form text
/// and may legally carry a `/` or a space, neither of which survives being
/// spliced into a URL path. A `:` is kept, encoded as [`canonical_segment`]
/// spells it, so a `urn:uuid:` stays recognisable.
fn sanitize(uid: &str) -> String {
    uid.chars()
        .map(|char| match char {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => String::from(char),
            ':' => String::from("%3A"),
            _ => String::from('-'),
        })
        .collect()
}

/// Whether the exchange died on an already-closed connection, the one failure
/// [`DavClient::op`] repairs.
///
/// An end of stream says the request was never answered, a broken pipe or
/// reset that it was never written: neither leaves a half-applied write.
fn is_connection_closed(err: &WebdavClientStdError) -> bool {
    match err {
        WebdavClientStdError::Send(WebdavSendError::Send(Http11SendError::Eof))
        | WebdavClientStdError::WebdavFollowRedirects(WebdavFollowRedirectsError::Send(
            Http11SendError::Eof,
        ))
        | WebdavClientStdError::WebdavSyncCollection(WebdavSyncCollectionError::Send(
            WebdavSendError::Send(Http11SendError::Eof),
        )) => true,
        WebdavClientStdError::Io(err) => matches!(
            err.kind(),
            ErrorKind::BrokenPipe | ErrorKind::ConnectionAborted | ErrorKind::ConnectionReset
        ),
        _ => false,
    }
}

/// The HTTP status an exchange failed with, wherever it surfaces, which
/// [`Request::retries`] reads as a throttle or not: 429, or 503 (RFC 9110
/// §15.6.4) for an idempotent exchange.
///
/// io-webdav keeps no `Retry-After`, so the wait is neverest's back-off.
fn throttle_status(err: &WebdavClientStdError) -> Option<u16> {
    match err {
        WebdavClientStdError::Send(WebdavSendError::HttpStatus { status, .. })
        | WebdavClientStdError::WebdavSyncCollection(WebdavSyncCollectionError::Send(
            WebdavSendError::HttpStatus { status, .. },
        ))
        | WebdavClientStdError::WebdavFollowRedirects(WebdavFollowRedirectsError::HttpStatus {
            status,
            ..
        }) => Some(*status),
        _ => None,
    }
}

/// The status a plain request failed with, as io-webdav surfaces a non-2xx
/// answer to anything but a report.
fn http_status(err: &WebdavClientStdError) -> Option<u16> {
    match err {
        WebdavClientStdError::Send(WebdavSendError::HttpStatus { status, .. }) => Some(*status),
        _ => None,
    }
}

/// Whether a `MOVE` was answered as one the server cannot do (pimdir SYNC
/// §4): not allowed there (405), not implemented (501), or the member not
/// found (404), Google's answer for a member it holds. A duplicate `UID` is
/// another variant, never one of these.
fn is_unsupported_move(err: &WebdavClientStdError) -> bool {
    matches!(http_status(err), Some(405 | 501 | 404))
}

/// Whether a request found its resource gone (404, 410).
fn is_gone(err: &WebdavClientStdError) -> bool {
    matches!(http_status(err), Some(404 | 410))
}

/// Whether a write was refused for `no-uid-conflict` (RFC 4791 §5.3.2, RFC
/// 6352 §6.3.2), the collection already holding that `UID`.
///
/// The error crosses the client seam as an [`anyhow::Error`], so the typed
/// refusal is read back out of its chain.
pub fn is_duplicate_uid(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<WebdavClientStdError>()
            .is_some_and(WebdavClientStdError::is_duplicate_uid)
    })
}

/// Whether a calendar takes tasks by its `supported-calendar-component-set`
/// (RFC 4791 §5.2.3): `None` for no set stated, which takes any component,
/// else whether the set names `VTODO`.
fn takes_tasks(components: &BTreeSet<String>) -> Option<bool> {
    if components.is_empty() {
        return None;
    }
    let vtodo = components
        .iter()
        .any(|component| component.eq_ignore_ascii_case("VTODO"));
    Some(vtodo)
}

/// Whether the server advertises `sync-collection` for a collection.
///
/// A collection nobody listed counts as supporting it: a server that does not
/// implement the report names the refusal itself, so the unknown case costs one
/// failed REPORT and never a wrong enumeration.
fn has_sync_collection(reports: Option<&BTreeSet<String>>) -> bool {
    match reports {
        Some(reports) => reports.contains(SYNC_COLLECTION.local),
        None => true,
    }
}

/// Whether the server rejected the sync token, the one enumeration failure
/// that is recoverable by falling back to a full report.
fn is_invalid_sync_token(err: &WebdavClientStdError) -> bool {
    matches!(
        err,
        WebdavClientStdError::WebdavSyncCollection(WebdavSyncCollectionError::InvalidSyncToken)
    )
}

#[cfg(test)]
impl DavClient {
    /// A session of `kind` over a scripted server answering `replies` in
    /// order, its home set `home` already discovered, for a test of this
    /// module or another driving a DAV source request by request.
    pub(crate) fn scripted(
        kind: DavKind,
        home: &str,
        replies: Vec<String>,
    ) -> (Self, scripted::Wire) {
        let base = Url::parse("https://dav.example.org/").unwrap();
        let wire = scripted::Wire::new(replies);
        let auth = WebdavAuth::None;
        let mut inner = WebdavClientStd::new(wire.stream(), auth.clone(), base.clone());
        let home = Some(base.join(home).unwrap());
        match kind {
            DavKind::Card => inner.addressbook_home_set = home,
            DavKind::Cal => inner.calendar_home_set = home,
        }
        let client = Self {
            kind,
            inner,
            // NOTE: a port nothing listens on, should a test run its script
            // dry and the session reconnect.
            server: Url::parse("http://127.0.0.1:1/").unwrap(),
            tls: Tls::default(),
            auth,
            throttle: Arc::new(Throttle::new("dav")),
        };
        (client, wire)
    }
}

/// A scripted DAV server: one connection answering its requests from a
/// script, in order, and recording each request written to it.
#[cfg(test)]
pub(crate) mod scripted {
    use std::{
        collections::VecDeque,
        io::{self, Read, Write},
        sync::{Arc, Mutex},
    };

    /// An answer of `status`, carrying `headers` and no body.
    pub(crate) fn reply(status: &str, headers: &[(&str, &str)]) -> String {
        answer(status, headers, "")
    }

    /// An answer of `status`, carrying `headers` and `body`.
    pub(crate) fn answer(status: &str, headers: &[(&str, &str)], body: &str) -> String {
        let headers: String = headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}\r\n"))
            .collect();
        format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    #[derive(Default)]
    struct State {
        replies: VecDeque<Vec<u8>>,
        out: VecDeque<u8>,
        writing: Vec<u8>,
        requests: Vec<String>,
    }

    /// The server's side of the connection.
    #[derive(Clone, Default)]
    pub(crate) struct Wire(Arc<Mutex<State>>);

    impl Wire {
        pub(crate) fn new(replies: Vec<String>) -> Self {
            let wire = Self::default();
            let mut state = wire.0.lock().unwrap();
            state.replies = replies.into_iter().map(String::into_bytes).collect();
            drop(state);
            wire
        }

        pub(crate) fn stream(&self) -> Stream {
            Stream(self.clone())
        }

        /// Every request written so far, head and body, in order.
        pub(crate) fn requests(&self) -> Vec<String> {
            let mut state = self.0.lock().unwrap();
            flush(&mut state);
            state.requests.clone()
        }

        /// The request line of each request, in order.
        pub(crate) fn lines(&self) -> Vec<String> {
            self.requests()
                .iter()
                .map(|request| request.lines().next().unwrap_or_default().to_owned())
                .collect()
        }
    }

    /// Closes the request being written once its answer is read.
    fn flush(state: &mut State) {
        if !state.writing.is_empty() {
            let request = String::from_utf8_lossy(&state.writing).into_owned();
            state.requests.push(request);
            state.writing.clear();
        }
    }

    pub(crate) struct Stream(Wire);

    impl Read for Stream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let mut state = (self.0).0.lock().unwrap();
            flush(&mut state);
            if state.out.is_empty() {
                // NOTE: a script run dry ends the stream, as a server
                // closing the connection.
                let Some(reply) = state.replies.pop_front() else {
                    return Ok(0);
                };
                state.out.extend(reply);
            }
            let n = buf.len().min(state.out.len());
            for (slot, byte) in buf.iter_mut().zip(state.out.drain(..n)) {
                *slot = byte;
            }
            Ok(n)
        }
    }

    impl Write for Stream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut state = (self.0).0.lock().unwrap();
            state.writing.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_calendar_url_is_keyed_as_a_listing_keys_it() {
        let home = "/dav/cal/test%40pimalaya.org/";
        assert_eq!(
            collection_key(home, "/dav/cal/test%40pimalaya.org/default/").as_deref(),
            Some("default")
        );
        assert_eq!(collection_key("/cal", "/cal/work").as_deref(), Some("work"));
        assert_eq!(collection_key("/cal/", "/"), None);
    }

    /// A calendar outside the home set is keyed by its path, as io-webdav
    /// lists it: Google names every calendar `events` under its own path, so
    /// a last segment would make the default calendar any of them.
    #[test]
    fn a_default_calendar_outside_the_home_set_is_keyed_by_its_path() {
        let home = "/caldav/v2/alice%40g/";
        assert_eq!(
            collection_key(home, "/caldav/v2/x%40g/events/").as_deref(),
            Some("/caldav/v2/x%40g/events")
        );
        assert_eq!(
            collection_key(home, "/caldav/v2/alice@g/events/").as_deref(),
            Some("events")
        );
    }

    #[test]
    fn a_member_href_addresses_by_its_last_segment() {
        assert_eq!(href_id("/dav/books/default/card-1.vcf"), "card-1.vcf");
        assert_eq!(
            href_id("https://dav.example.org/books/default/card-1.vcf"),
            "card-1.vcf"
        );

        assert_eq!(href_id("/dav/books/default/"), "default");
        assert_eq!(href_id("card-1.vcf"), "card-1.vcf");
    }

    /// A `UID` a server lists encoded is the resource neverest created under
    /// it, not a second one.
    #[test]
    fn an_encoded_colon_names_the_resource_created_with_it() {
        let created = resource_id(DavKind::Card, stated("urn:uuid:4fbe8971-0bc3"), b"");
        assert_eq!(
            href_id("/dav/card/test/default/urn%3Auuid%3A4fbe8971-0bc3.vcf"),
            created
        );
        assert_eq!(
            href_id("/dav/card/test/default/urn%3auuid%3a4fbe8971-0bc3.vcf"),
            created
        );
        // NOTE: a server listing the colon literally names it too.
        assert_eq!(
            href_id("/dav/card/test/default/urn:uuid:4fbe8971-0bc3.vcf"),
            created
        );
        assert_eq!(href_id("/books/card%2D1%2Evcf"), "card-1.vcf");

        // NOTE: decoding these would name another resource, or none.
        assert_eq!(href_id("/books/a%2fb.vcf"), "a%2Fb.vcf");
        assert_eq!(href_id("/books/a%20b.vcf"), "a%20b.vcf");
        assert_eq!(href_id("/books/100%25.vcf"), "100%25.vcf");
        assert_eq!(href_id("/books/bad%zz%4.vcf"), "bad%zz%4.vcf");
        assert_eq!(href_id("/books/é:.vcf"), "é%3A.vcf");
    }

    /// The link id of an item whose content states its identity, as
    /// [`Kind::split_link_id`] reads it.
    fn stated(uid: &str) -> LinkId<'_> {
        LinkId {
            hint: Some(uid),
            mint: None,
        }
    }

    #[test]
    fn a_new_resource_is_addressed_by_its_uid_under_its_kinds_extension() {
        assert_eq!(
            resource_id(DavKind::Card, stated("card-1"), b""),
            "card-1.vcf"
        );
        assert_eq!(
            resource_id(DavKind::Cal, stated("event-1"), b""),
            "event-1.ics"
        );

        assert_eq!(
            resource_id(DavKind::Card, stated("urn:uuid:4fbe8971-0bc3"), b""),
            "urn%3Auuid%3A4fbe8971-0bc3.vcf"
        );
        assert_eq!(
            resource_id(DavKind::Card, stated("a b/c"), b""),
            "a-b-c.vcf"
        );
    }

    /// Two items sharing a `UID` reach two hrefs: the minted part of the second
    /// one's key keeps its `PUT` off the resource the first already holds.
    #[test]
    fn a_minted_copy_is_addressed_beside_its_twin_rather_than_over_it() {
        let twin = stated("event-1@google.com");
        let copy = LinkId {
            hint: Some("event-1@google.com"),
            mint: Some("event-1%2540google.com.ics"),
        };

        let twin = resource_id(DavKind::Cal, twin, b"");
        let copy = resource_id(DavKind::Cal, copy, b"");

        assert_eq!(twin, "event-1-google.com.ics");
        assert_eq!(copy, "event-1-google.com-event-1-2540google.com.ics.ics");
        assert_ne!(twin, copy);
    }

    /// Two bodies that hash the same are what got the pair minted, so a
    /// body-derived name would be the collision itself.
    #[test]
    fn a_minted_copy_without_a_uid_is_never_named_after_its_body() {
        let card = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:No Uid\r\nEND:VCARD\r\n";
        let copy = LinkId {
            hint: None,
            mint: Some("card-2.vcf"),
        };

        let twin = resource_id(DavKind::Card, LinkId::default(), card);
        let copy = resource_id(DavKind::Card, copy, card);

        assert_eq!(copy, "card-2.vcf.vcf");
        assert_ne!(twin, copy);
    }

    #[test]
    fn a_resource_without_a_uid_is_addressed_by_its_body_digest() {
        let card = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:No Uid\r\nEND:VCARD\r\n";
        let event =
            b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:No Uid\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

        for (kind, body, extension) in [
            (DavKind::Card, card.as_slice(), ".vcf"),
            (DavKind::Cal, event.as_slice(), ".ics"),
        ] {
            let id = resource_id(kind, LinkId::default(), body);

            assert!(id.ends_with(extension), "got {id}");
            assert!(!id.starts_with("hash:"), "the prefix is not a path segment");
            assert_eq!(id, resource_id(kind, LinkId::default(), body));
        }
    }

    /// RFC 6578 is an extension, so a collection advertising no
    /// `sync-collection` is listed instead, decided before any REPORT is sent.
    #[test]
    fn a_collection_without_the_report_is_listed_instead() {
        let advertised = |reports: &[&str]| {
            reports
                .iter()
                .map(|report| String::from(*report))
                .collect::<BTreeSet<String>>()
        };

        assert!(!has_sync_collection(Some(&advertised(&[
            "addressbook-multiget",
            "addressbook-query",
        ]))));
        assert!(!has_sync_collection(Some(&advertised(&[
            "calendar-multiget",
            "calendar-query",
        ]))));
        assert!(has_sync_collection(Some(&advertised(&[
            "calendar-query",
            "sync-collection",
        ]))));

        assert!(
            has_sync_collection(None),
            "a collection nobody listed is enumerated by the report, whose refusal names itself",
        );
    }

    /// A component set leaving VTODO out is an events-only calendar; no set
    /// at all takes any component (RFC 4791 §5.2.3).
    #[test]
    fn a_calendar_takes_tasks_as_its_component_set_says() {
        let set = |components: &[&str]| {
            components
                .iter()
                .map(|component| String::from(*component))
                .collect::<BTreeSet<String>>()
        };

        assert_eq!(takes_tasks(&set(&["VEVENT"])), Some(false));
        assert_eq!(takes_tasks(&set(&["VEVENT", "VJOURNAL"])), Some(false));
        assert_eq!(takes_tasks(&set(&["VEVENT", "VTODO"])), Some(true));
        assert_eq!(takes_tasks(&set(&["vtodo"])), Some(true));
        assert_eq!(takes_tasks(&set(&[])), None);
    }

    use scripted::{answer, reply};

    /// A calendar session over `replies`, its home set `/dav/cal/`.
    fn calendars(replies: Vec<String>) -> (DavClient, scripted::Wire) {
        DavClient::scripted(DavKind::Cal, "/dav/cal/", replies)
    }

    /// Whether `request` carries the header line `line`, names compared
    /// without case.
    fn carries(request: &str, line: &str) -> bool {
        let line = line.to_ascii_lowercase();
        request
            .lines()
            .any(|header| header.to_ascii_lowercase() == line)
    }

    /// A move is one server-side `MOVE`, keeping the id, refusing to
    /// overwrite and gated on the source's ETag.
    #[test]
    fn an_item_moves_by_one_server_side_move() {
        let (mut dav, wire) = calendars(vec![reply("201 Created", &[("ETag", "\"m1\"")])]);

        let moved = dav
            .move_item("tasks", "work", "t1.ics", Some("\"e1\""))
            .unwrap()
            .expect("a CalDAV server moves");

        assert_eq!(moved.id, "t1.ics");
        assert_eq!(moved.revision.as_deref(), Some("m1"));
        let requests = wire.requests();
        assert_eq!(wire.lines(), ["MOVE /dav/cal/tasks/t1.ics HTTP/1.1"]);
        let request = &requests[0];
        assert!(
            carries(
                request,
                "destination: https://dav.example.org/dav/cal/work/t1.ics"
            ),
            "{request}"
        );
        assert!(carries(request, "overwrite: F"), "{request}");
        assert!(carries(request, "if-match: \"e1\""), "{request}");
    }

    /// A server answering the `MOVE` as one it cannot do has the item
    /// uploaded instead; any other refusal is the caller's to report.
    #[test]
    fn a_move_the_server_cannot_do_is_answered_none() {
        for status in [
            "405 Method Not Allowed",
            "501 Not Implemented",
            "404 Not Found",
        ] {
            let (mut dav, _) = calendars(vec![reply(status, &[])]);
            let moved = dav.move_item("tasks", "work", "t1.ics", None).unwrap();
            assert!(moved.is_none(), "{status}");
        }

        let (mut dav, _) = calendars(vec![reply("412 Precondition Failed", &[])]);
        let err = dav.move_item("tasks", "work", "t1.ics", None).unwrap_err();
        assert!(!is_duplicate_uid(&err), "{err:#}");

        let refusal = "<d:error xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\">\
                       <c:no-uid-conflict/></d:error>";
        let (mut dav, _) = calendars(vec![answer(
            "403 Forbidden",
            &[("Content-Type", "application/xml")],
            refusal,
        )]);
        let err = dav.move_item("tasks", "work", "t1.ics", None).unwrap_err();
        assert!(is_duplicate_uid(&err), "{err:#}");
    }

    /// io-webdav moves no card: CardDAV answers none without a request.
    #[test]
    fn a_card_is_never_moved_server_side() {
        let (mut dav, wire) = DavClient::scripted(DavKind::Card, "/dav/card/", Vec::new());
        let moved = dav.move_item("a", "b", "c1.vcf", None).unwrap();
        assert!(moved.is_none());
        assert!(wire.requests().is_empty());
    }

    /// A relocation is a `MOVE`, never a create then a delete, which a
    /// server keeping one `UID` per account (Fastmail) refuses.
    #[test]
    fn a_relocation_moves_rather_than_recreates() {
        let (mut dav, wire) = calendars(vec![reply("201 Created", &[])]);

        dav.move_items("tasks", "work", &["t1.ics"]).unwrap();

        assert_eq!(wire.lines(), ["MOVE /dav/cal/tasks/t1.ics HTTP/1.1"]);
    }

    /// A server with no `MOVE` (Google answers 404) has the item re-created
    /// at the target, then deleted from the source on its ETag.
    #[test]
    fn a_relocation_recreates_where_the_server_cannot_move() {
        let body = "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n";
        let (mut dav, wire) = calendars(vec![
            reply("404 Not Found", &[]),
            answer("200 OK", &[("ETag", "\"e1\"")], body),
            reply("201 Created", &[("ETag", "\"n1\"")]),
            reply("204 No Content", &[]),
        ]);

        dav.move_items("tasks", "work", &["t1.ics"]).unwrap();

        assert_eq!(
            wire.lines(),
            [
                "MOVE /dav/cal/tasks/t1.ics HTTP/1.1",
                "GET /dav/cal/tasks/t1.ics HTTP/1.1",
                "PUT /dav/cal/work/t1.ics HTTP/1.1",
                "DELETE /dav/cal/tasks/t1.ics HTTP/1.1",
            ]
        );
    }

    /// A delete finding its member gone is done: a create delivered by a
    /// move took it along (pimdir SYNC §4).
    #[test]
    fn a_delete_of_a_member_already_gone_is_done() {
        for status in ["404 Not Found", "410 Gone"] {
            let (mut dav, _) = calendars(vec![reply(status, &[])]);
            dav.delete_item("tasks", "t1.ics", Some("\"e1\"")).unwrap();
        }

        let (mut dav, _) = calendars(vec![reply("412 Precondition Failed", &[])]);
        assert!(dav.delete_item("tasks", "t1.ics", Some("\"e1\"")).is_err());
    }

    /// A calendar outside the home set is addressed by its path, the id
    /// io-webdav lists it under.
    #[test]
    fn a_calendar_outside_the_home_set_is_addressed_by_its_path() {
        let (mut dav, wire) = DavClient::scripted(
            DavKind::Cal,
            "/caldav/v2/alice%40g/",
            vec![reply("201 Created", &[])],
        );

        dav.move_items("/caldav/v2/x%40g/events", "events", &["t1.ics"])
            .unwrap();

        let request = &wire.requests()[0];
        assert_eq!(
            wire.lines(),
            ["MOVE /caldav/v2/x%40g/events/t1.ics HTTP/1.1"]
        );
        assert!(
            carries(
                request,
                "destination: https://dav.example.org/caldav/v2/alice%40g/events/t1.ics"
            ),
            "{request}"
        );
    }
}
