//! # DAV client
//!
//! [`DavClient`] wraps the std blocking io-webdav client behind the same
//! adapter surface as the IMAP and Graph backends, serving CardDAV (RFC 6352)
//! and CalDAV (RFC 4791) alike.
//!
//! One adapter serves both, since they differ only in the home set they
//! discover, the collection they list and a new resource's extension
//! ([`DavKind`]). This is the mutable-content side of the sync, where the ETag
//! and `If-Match` plumbing built for mail is exercised.
//!
//! Three shapes differ from the mail backends: a collection is keyed by its
//! path segment under the home set as served, else by its path (a display name
//! is optional, mutable and free to collide); items resolve at `Full` only, a
//! report carrying no `UID`; flags are known-empty, not unknown.
//!
//! Enumeration is RFC 6578 where the server implements it: a rejected token
//! ([`WebdavSyncCollectionError::InvalidSyncToken`]) gets a fresh full report,
//! and a truncated one (§3.6) is drained from the token it returned.
//!
//! A server implementing none of it refuses with the RFC 3253 §3.6
//! `DAV:supported-report` precondition: such a collection is listed by
//! `PROPFIND`, the same ids and ETags with no token, rather than not synced.

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
    rfc4791::calendar::{CALDAV, CaldavCalendar, CaldavCalendarPatch},
    rfc4918::{
        WebdavAuth, WebdavProperty, collection_id, follow_redirects::WebdavFollowRedirectsError,
        propfind::WebdavPropfind, resolve_href, send::WebdavSendError,
    },
    rfc6352::addressbook::{CarddavAddressbook, CarddavAddressbookPatch},
    rfc6578::sync_collection::{
        SYNC_COLLECTION, WebdavSyncChange, WebdavSyncCollectionError, WebdavSyncCollectionOptions,
        WebdavSyncDelta,
    },
};
use log::{debug, warn};
use pimalaya_stream::{proxy::Proxy, tls::Tls};
use url::Url;

use crate::{
    account::{DavAccount, DavEndpoint},
    client::{EnumEntry, Enumeration, Gone, WrittenItem},
    collection::Collection,
    error::{ErrorCode, names_the_request},
    flag::Flag,
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
    #[cfg_attr(not(feature = "caldav"), allow(dead_code))]
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
    proxy: Proxy,
    /// The source's back-off, shared by its connections.
    throttle: Arc<Throttle>,
    /// The sync token each collection of the last listing stated (RFC 6578
    /// §4), by key.
    tokens: BTreeMap<String, String>,
}

impl DavClient {
    /// Opens the session and discovers the home set, so a misconfigured URL or
    /// credential fails here rather than at the first enumeration.
    ///
    /// A home set the account names is taken as it is, nothing discovered.
    pub fn connect(account: &DavAccount, throttle: Arc<Throttle>) -> Result<Self> {
        let DavAccount {
            kind,
            endpoint,
            tls,
            auth,
            proxy,
        } = account;
        let kind = *kind;
        let server = endpoint.url();
        let options = connect_options(tls, proxy);
        let inner = WebdavClientStd::connect(server, auth.clone(), options)
            .with_context(|| format!("Cannot connect to the {kind} server"))?;
        let mut client = Self {
            kind,
            inner,
            server: server.clone(),
            tls: tls.clone(),
            auth: auth.clone(),
            proxy: proxy.clone(),
            throttle,
            tokens: BTreeMap::new(),
        };

        if let DavEndpoint::Home(home) = endpoint {
            match kind {
                DavKind::Card => client.inner.addressbook_home_set = Some(home.clone()),
                DavKind::Cal => client.inner.calendar_home_set = Some(home.clone()),
            }
            debug!("{kind} home set, as configured: {home}");
            return Ok(client);
        }

        let home = match kind {
            DavKind::Card => client.op(WebdavClientStd::addressbook_home_set),
            DavKind::Cal => client.op(WebdavClientStd::calendar_home_set),
        }
        .with_context(|| format!("Cannot discover the {kind} home set"));
        let home = anonymous_hint(&client.auth, home)?;
        debug!("{kind} home set: {home}");

        Ok(client)
    }

    /// The media type of the items this session syncs.
    pub fn media_type(&self) -> &'static str {
        self.kind.media_type()
    }

    /// Runs one WebDAV exchange, reopening a connection the server had closed.
    ///
    /// io-webdav holds one stream, which an HTTP/1.0 or `Connection: close`
    /// peer breaks after an answer: only that end-of-stream failure retries,
    /// never an applied write. The [`Throttle`] resends on 429, 503.
    fn op<T>(
        &mut self,
        run: impl FnMut(&mut WebdavClientStd) -> Result<T, WebdavClientStdError>,
    ) -> Result<T, WebdavClientStdError> {
        self.send(Request::Idempotent, run)
    }

    /// Runs one WebDAV exchange creating a resource or a collection.
    ///
    /// Sent again on a 429 alone, answered before anything is applied, never on
    /// a 503, after which the resource may exist.
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
        let options = connect_options(&self.tls, &self.proxy);
        let mut inner = WebdavClientStd::connect(&self.server, self.auth.clone(), options)?;
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
    /// Counts are never reported: DAV has no cheap total. A collection's id is
    /// its key, its display name free to collide, change or be absent; the name
    /// still shows, falling back to the key (often a server's UUID).
    pub fn list_collections(&mut self, _with_counts: bool) -> Result<Vec<Collection>> {
        let kind = self.kind;
        let mut tokens = BTreeMap::new();
        let mut token = |key: &str, token: Option<String>| {
            if let Some(token) = token {
                tokens.insert(key.to_owned(), token);
            }
        };
        let collections: Result<BTreeMap<String, Listed>> = match kind {
            DavKind::Card => self.op(WebdavClientStd::list_addressbooks).map(|books| {
                books
                    .into_iter()
                    .map(|book| {
                        let key = listed_key(book.id, &book.href);
                        token(&key, book.sync_token);
                        (key, (book.display_name, None))
                    })
                    .collect()
            }),
            DavKind::Cal => self.op(WebdavClientStd::list_calendars).map(|calendars| {
                calendars
                    .into_iter()
                    .map(|calendar| {
                        let tasks = takes_tasks(&calendar.components);
                        let key = listed_key(calendar.id, &calendar.href);
                        token(&key, calendar.sync_token);
                        (key, (calendar.display_name, tasks))
                    })
                    .collect()
            }),
        }
        .with_context(|| format!("Cannot list the {kind} collections"));
        self.tokens = tokens;
        let collections = anonymous_hint(&self.auth, collections)?;

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

    /// The collections of `settled`, by name and checkpoint, whose sync
    /// token in the last listing is that checkpoint (RFC 6578 §4).
    ///
    /// Nothing changed there since, so each stands at it still.
    pub fn quiet_collections(&self, settled: &[(String, String)]) -> Vec<(String, String)> {
        settled
            .iter()
            .filter(|(key, checkpoint)| {
                !checkpoint.is_empty() && self.tokens.get(key) == Some(checkpoint)
            })
            .cloned()
            .collect()
    }

    /// The collection a new item goes to when none is named, by id.
    ///
    /// For CalDAV, the calendar the scheduling inbox names as
    /// `schedule-default-calendar-URL` (RFC 6638 §9.2), if any; CardDAV names
    /// none. A check-only probe: a failed lookup is no default, never an error.
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
            let coroutine: WebdavPropfind =
                WebdavPropfind::new(&dav.base_url, dav.auth(), &dav.user_agent, path, 0, &[prop]);
            dav.run(coroutine)
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

    /// Displays the collection keyed `collection` as `name`, its key kept:
    /// a `PROPPATCH` of `DAV:displayname` alone.
    pub fn rename_collection(&mut self, collection: &str, name: &str) -> Result<()> {
        let kind = self.kind;
        let name = Some(name.to_owned());

        match kind {
            DavKind::Card => {
                let patch = CarddavAddressbookPatch {
                    id: collection.to_owned(),
                    display_name: Some(name),
                    description: None,
                    color: None,
                };
                self.op(|dav| dav.update_addressbook(&patch))
            }
            DavKind::Cal => {
                let patch = CaldavCalendarPatch {
                    id: collection.to_owned(),
                    display_name: Some(name),
                    description: None,
                    color: None,
                    tz: None,
                };
                self.op(|dav| dav.update_calendar(&patch))
            }
        }
        .with_context(|| format!("Cannot rename the {kind} collection {collection}"))
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
    /// RFC 6578 is an extension: a collection refused with the RFC 3253 §3.6
    /// `DAV:supported-report` precondition is listed instead, its checkpoint
    /// empty, which reads back as no cursor.
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

    /// The reports the server advertises for a collection.
    ///
    /// Read from the `supported-report-set` io-webdav caches while listing,
    /// under the id [`webdav_id`] gives the key.
    fn reports(&self, collection: &str) -> Option<&BTreeSet<String>> {
        let id = webdav_id(collection);
        match self.kind {
            DavKind::Card => self.inner.addressbook_reports.get(&id),
            DavKind::Cal => self.inner.calendar_reports.get(&id),
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
    /// No token, so every run lists it whole; truncated or naming a member
    /// with an error, it is incomplete. Not the query report: its filter
    /// parses every resource, one unparsable failing all.
    fn list(&mut self, collection: &str) -> Result<Enumeration, WebdavClientStdError> {
        let opts = WebdavSyncCollectionOptions { fallback: true };
        let delta = self.sync_collection(collection, None, opts)?;

        if delta.truncated {
            warn!(
                "{} server truncated the listing of {collection}, reconciling as a delta",
                self.kind
            );
        }
        let errored = errored(&delta);
        self.warn_errored(collection, &errored);

        Ok(Enumeration {
            items: delta.changed.into_iter().map(entry).collect(),
            vanished: Vec::new(),
            complete: !delta.truncated && errored.is_empty(),
            truncated: delta.truncated,
            errored,
            checkpoint: Some(Vec::new()),
            ..Default::default()
        })
    }

    /// Warns of the members `collection`'s listing named with an error.
    fn warn_errored(&self, collection: &str, errored: &[(String, Option<u16>)]) {
        for (id, status) in errored {
            warn!(
                "{} server listed {id} of {collection} with status {status:?}, \
                 reconciling as a delta",
                self.kind
            );
        }
    }

    /// One enumeration, draining every truncated round into a single result.
    ///
    /// A listing still truncated, or naming a member with an error, is
    /// incomplete, lest a member read as deleted (RFC 6578 §3.6); with an
    /// error its checkpoint stays put, the token being past that change.
    fn sync(
        &mut self,
        collection: &str,
        token: Option<&str>,
    ) -> Result<Enumeration, WebdavClientStdError> {
        let first = token.is_none();
        let start = token.unwrap_or_default().as_bytes().to_vec();
        let mut items = Vec::new();
        let mut vanished = Vec::new();
        let mut errored = Vec::new();
        let mut token = token.map(String::from);
        let mut truncated = false;

        for _ in 0..MAX_SYNC_ROUNDS {
            let delta = self.sync_collection(collection, token.as_deref(), Default::default())?;

            errored.extend(self::errored(&delta));
            items.extend(delta.changed.into_iter().map(entry));
            vanished.extend(delta.vanished.iter().map(|href| href_id(href)));
            truncated = delta.truncated;

            if truncated && delta.sync_token.is_none() {
                break;
            }
            token = delta.sync_token;
            if !truncated {
                break;
            }
        }

        if truncated {
            warn!(
                "{} server truncated the listing of {collection}, reconciling as a delta",
                self.kind
            );
        }
        self.warn_errored(collection, &errored);

        let checkpoint = match errored.is_empty() {
            true => token.unwrap_or_default().into_bytes(),
            false => start,
        };

        Ok(Enumeration {
            items,
            vanished,
            complete: first && !truncated && errored.is_empty(),
            truncated,
            errored,
            checkpoint: Some(checkpoint),
            ..Default::default()
        })
    }

    /// Batch-fetches bodies in one multiget round-trip, routing each into the
    /// sink the caller opens for it.
    ///
    /// Answers the ids the server said are gone (404, 410), named by
    /// [`href_id`] as the enumeration names members; any other unreadable row
    /// stays unanswered, for the caller's lone read to report.
    pub fn fetch_bodies<S: Write>(
        &mut self,
        collection: &str,
        ids: &[&str],
        mut open: impl FnMut(&str) -> std::io::Result<S>,
        mut done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<Vec<String>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let kind = self.kind;
        let (objects, unreadable) = match kind {
            DavKind::Card => self
                .op(|dav| dav.multiget_cards(collection, ids))
                .map(|ok| {
                    let cards = ok
                        .cards
                        .into_iter()
                        .map(|card| (href_id(&card.href), card.etag, card.data))
                        .collect::<Vec<_>>();
                    (cards, ok.unreadable)
                }),
            DavKind::Cal => self
                .op(|dav| dav.multiget_items(collection, ids))
                .map(|ok| {
                    let items = ok
                        .items
                        .into_iter()
                        .map(|item| (href_id(&item.href), item.etag, item.data))
                        .collect::<Vec<_>>();
                    (items, ok.unreadable)
                }),
        }
        .with_context(|| format!("Cannot fetch {kind} bodies from {collection}"))?;

        for (id, etag, data) in objects {
            let mut sink = open(&id)?;
            sink.write_all(&data)?;

            done(&id, etag.as_deref(), sink)?;
        }

        let mut gone = Vec::new();
        for entry in unreadable {
            match entry.status {
                Some(404 | 410) => gone.push(href_id(&entry.href)),
                status => debug!(
                    "{kind} multiget of {collection} could not read {} (status {status:?})",
                    entry.href,
                ),
            }
        }

        Ok(gone)
    }

    /// Streams one resource's raw bytes, returning the ETag they correspond to.
    ///
    /// A resource the server answers missing (404, 410) fails with [`Gone`]:
    /// a member every read finds missing is owed nothing more in the run,
    /// rather than failing its collection's fetch in every run.
    pub fn get_item_stream(
        &mut self,
        collection: &str,
        id: &str,
        mut sink: impl Write,
    ) -> Result<Option<String>> {
        let (data, etag) = match self.read(collection, id) {
            Ok(read) => read,
            Err(err) if is_gone(&err) => {
                return Err(anyhow::Error::new(err).context(Gone(format!("{id} in {collection}"))));
            }
            Err(err) => {
                return Err(
                    anyhow::Error::new(err).context(format!("Cannot read {id} in {collection}"))
                );
            }
        };

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
    /// Each goes by `MOVE` ([`move_item`](Self::move_item)): Fastmail refuses a
    /// create while the source holds the `UID`. Else it is re-created at the
    /// target, the source deleted once that is accepted.
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
    /// Answers the moved resource and its ETag, or `None` when the server
    /// cannot move it: CardDAV, a 405, 501 or 404 (Google's for a held member,
    /// pimdir SYNC §4). Other refusals err, [`is_duplicate_uid`] among them.
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

        match self.op_create(|dav| dav.move_item(from, id, to, if_match)) {
            Ok(moved) => Ok(Some(WrittenItem {
                id: served_segment(&moved.href).to_owned(),
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

    /// Rejected: DAV has no flags.
    ///
    /// The enumeration reports every member known-empty, and a flag kept only
    /// in the store is never pushed, the flag right withheld for non-mail
    /// kinds.
    pub fn update_flags(&mut self, _ids: &[&str], _add: &[Flag], _remove: &[Flag]) -> Result<()> {
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
                    id: served_segment(&created.href).to_owned(),
                    revision: created.etag,
                }),
            DavKind::Cal => self
                .op_create(|dav| dav.create_item(collection, id, body.clone()))
                .map(|created| WrittenItem {
                    id: served_segment(&created.href).to_owned(),
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

/// The key a listing gives the collection at `path` under the home set `home`,
/// as [`listed_key`] keys a calendar.
///
/// Its last segment directly under the home set, as served, else its path with
/// a leading slash (Google lists calendars as `/caldav/v2/<calendar>/events/`).
fn collection_key(home: &str, path: &str) -> Option<String> {
    let key = listed_key(collection_id(home, path), path);
    (!key.trim_matches('/').is_empty()).then_some(key)
}

/// The key of the collection io-webdav lists as `id` at `href`: a path id
/// as it stands, else the href's last segment as the server served it.
///
/// io-webdav decodes that segment (`My%20Cal` read as `My Cal`), as it
/// reads a raw `My Cal` another server serves: the segment is the key
/// io-webdav 0.9.1 gave, so no store key moves and the two stay apart.
fn listed_key(id: String, href: &str) -> String {
    match id.starts_with('/') {
        true => id,
        false => served_segment(href).to_owned(),
    }
}

/// The id io-webdav lists the collection keyed `key` under, keying its cache.
///
/// A segment is decoded as its listing decodes one, any other key kept.
fn webdav_id(key: &str) -> String {
    match key.contains(['/', '?', '#']) {
        true => key.to_owned(),
        false => collection_id("", key),
    }
}

/// The last path segment of `href` as served, query and fragment dropped.
///
/// It is the id io-webdav 0.9.1 read from an href, before it decoded one.
fn served_segment(href: &str) -> &str {
    let path = href.split(['?', '#']).next().unwrap_or(href);
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default()
}

/// The connect options of a session: the configured TLS and proxy.
fn connect_options(tls: &Tls, proxy: &Proxy) -> WebdavClientStdConnectOptions {
    WebdavClientStdConnectOptions {
        tls: tls.clone(),
        proxy: proxy.clone(),
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

/// The members `delta` listed with an error, each named by [`href_id`] as the
/// enumeration names members, with its status.
fn errored(delta: &WebdavSyncDelta) -> Vec<(String, Option<u16>)> {
    delta
        .unreadable
        .iter()
        .map(|row| (href_id(&row.href), row.status))
        .collect()
}

/// The addressing id of a member href: its last non-empty path segment, in the
/// one spelling [`canonical_segment`] gives.
///
/// A server may spell octets literally or encoded: Stalwart lists
/// `urn:uuid:x.vcf` as `urn%3Auuid%3Ax.vcf`, which once read as a second
/// resource and duplicated the item.
fn href_id(href: &str) -> String {
    let segment = href
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(href);
    canonical_segment(segment)
}

/// One spelling for a segment: an encoded unreserved octet decoded (RFC 3986
/// §6.2.2.2), a literal `:` encoded, every escape upper case.
///
/// A colon in a relative reference's first segment reads as a scheme (RFC 3986
/// §4.2): io-webdav got a 404 reading `urn:uuid:x.vcf` back. Other escapes
/// (`/`, space) stay encoded, decoding would name another resource.
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

/// Keeps a `UID` addressable as one path segment.
///
/// A `UID` is free-form and may carry a `/` or a space, which no URL path
/// survives. A `:` stays, encoded as [`canonical_segment`] spells it, so a
/// `urn:uuid:` stays recognisable.
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

/// The HTTP status an exchange failed with, wherever it surfaces.
///
/// [`Request::retries`] reads 429, or 503 for an idempotent exchange (RFC 9110
/// §15.6.4), as a throttle; io-webdav keeps no `Retry-After`, so the wait is
/// neverest's back-off.
fn throttle_status(err: &WebdavClientStdError) -> Option<u16> {
    refused(err).map(|(status, _)| status)
}

/// The status and body of an answer that refused an exchange, wherever
/// io-webdav surfaces it.
fn refused(err: &WebdavClientStdError) -> Option<(u16, &str)> {
    match err {
        WebdavClientStdError::Send(WebdavSendError::HttpStatus { status, body })
        | WebdavClientStdError::WebdavSyncCollection(WebdavSyncCollectionError::Send(
            WebdavSendError::HttpStatus { status, body },
        ))
        | WebdavClientStdError::WebdavFollowRedirects(WebdavFollowRedirectsError::HttpStatus {
            status,
            body,
        }) => Some((*status, body)),
        _ => None,
    }
}

/// The code of a DAV refusal in an error chain.
///
/// A 507 or `quota-not-exceeded` (RFC 4331 §6) is `quota-exceeded`, a 429 past
/// the waits `rate-limited`, a 403 naming `need-privileges` (RFC 3744 §7.1.1)
/// or nothing `permission-denied`, any other a failed precondition.
pub(crate) fn refusal_code(cause: &(dyn std::error::Error + 'static)) -> Option<ErrorCode> {
    let (status, body) = refused(cause.downcast_ref::<WebdavClientStdError>()?)?;

    match status {
        _ if body.contains("quota-not-exceeded") => Some(ErrorCode::QuotaExceeded),
        507 => Some(ErrorCode::QuotaExceeded),
        429 => Some(ErrorCode::RateLimited),
        403 if body.contains("need-privileges") || !names_precondition(body) => {
            Some(ErrorCode::PermissionDenied)
        }
        _ => None,
    }
}

/// Whether a DAV answer in an error chain refused the change as it stands.
///
/// A status naming the request ([`names_the_request`]), a failed precondition
/// (RFC 4791 §5.3.2) or a forbidden write (pimdir SYNC §4); a duplicate `UID`
/// is caught before, with its own report entry.
pub(crate) fn refuses(cause: &(dyn std::error::Error + 'static)) -> bool {
    cause
        .downcast_ref::<WebdavClientStdError>()
        .and_then(refused)
        .is_some_and(|(status, _)| names_the_request(status))
}

/// Whether a refusal's body is a `DAV:error` element, which names the
/// precondition the request failed (RFC 4918 §16).
fn names_precondition(body: &str) -> bool {
    body.contains(":error") || body.contains("<error")
}

/// A listed collection's display name, and whether it takes tasks.
type Listed = (Option<String>, Option<bool>);

/// `result`, a 401 answered to a session with no `auth` saying so: the
/// server asks for credentials the account never configured.
fn anonymous_hint<T>(auth: &WebdavAuth, result: Result<T>) -> Result<T> {
    match result {
        Err(err) if matches!(auth, WebdavAuth::None) && answered_401(&err) => {
            Err(err
                .context("The server asks for credentials, and the account configures no `auth`"))
        }
        result => result,
    }
}

/// Whether a request in an error chain was answered 401.
fn answered_401(err: &anyhow::Error) -> bool {
    err.chain()
        .filter_map(|cause| cause.downcast_ref::<WebdavClientStdError>())
        .any(|err| http_status(err) == Some(401))
}

/// The status a plain request failed with, as io-webdav surfaces a non-2xx
/// answer to anything but a report.
fn http_status(err: &WebdavClientStdError) -> Option<u16> {
    match err {
        WebdavClientStdError::Send(WebdavSendError::HttpStatus { status, .. }) => Some(*status),
        _ => None,
    }
}

/// Whether a `MOVE` was answered as one the server cannot do (pimdir SYNC §4).
///
/// Not allowed (405), not implemented (501), or not found (404), Google's
/// answer for a member it holds; a duplicate `UID` is never one of these.
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

/// Whether a calendar takes tasks, by its `supported-calendar-component-set`.
///
/// `None` for no set stated, taking any component (RFC 4791 §5.2.3), else
/// whether the set names `VTODO`.
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
    /// A session of `kind` over a scripted server answering `replies` in order.
    ///
    /// Its home set `home` is already discovered; the server URL is a port
    /// nothing listens on, should a script run dry and the session reconnect.
    pub(crate) fn scripted(
        kind: DavKind,
        home: &str,
        replies: Vec<String>,
    ) -> (Self, crate::scripted::Server) {
        let base = Url::parse("https://dav.example.org/").unwrap();
        let wire = crate::scripted::Server::replying(replies);
        let auth = WebdavAuth::None;
        let stream = wire.open().unwrap();
        let mut inner = WebdavClientStd::new(stream, auth.clone(), base.clone());
        let home = Some(base.join(home).unwrap());
        match kind {
            DavKind::Card => inner.addressbook_home_set = home,
            DavKind::Cal => inner.calendar_home_set = home,
        }
        let client = Self {
            kind,
            inner,
            server: Url::parse("http://127.0.0.1:1/").unwrap(),
            tls: Tls::default(),
            auth,
            proxy: Proxy::None,
            throttle: Arc::new(Throttle::new("dav")),
            tokens: BTreeMap::new(),
        };
        (client, wire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Failure;

    /// A server asking for credentials of a session configuring no `auth`
    /// says the account configures none, so a forgotten table reads as one.
    #[test]
    fn a_401_with_no_auth_names_the_missing_auth() {
        let unauthorized = String::from(
            "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"dav\"\r\n\
             Content-Length: 0\r\n\r\n",
        );
        let (mut dav, _wire) = DavClient::scripted(DavKind::Card, "/dav/card/", vec![unauthorized]);

        let err = dav.list_collections(false).unwrap_err();

        let message = format!("{err:#}");
        assert!(message.contains("configures no `auth`"), "{message}");
    }

    /// A configured home set is where the session lists, nothing discovered.
    ///
    /// The server here accepts the connection and never answers, so a discovery
    /// request would hang or fail.
    #[test]
    fn a_configured_home_set_is_taken_without_discovery() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let home = Url::parse(&format!("http://127.0.0.1:{port}/card/user/")).unwrap();
        let account = DavAccount {
            kind: DavKind::Card,
            endpoint: DavEndpoint::Home(home.clone()),
            tls: Tls::default(),
            auth: WebdavAuth::None,
            proxy: Proxy::None,
        };

        let client = DavClient::connect(&account, Arc::new(Throttle::new("carddav"))).unwrap();

        assert_eq!(client.inner.addressbook_home_set, Some(home));
        assert_eq!(client.inner.principal_url, None);
    }

    /// io-webdav decodes an escaped segment; the key stays the segment served.
    #[test]
    fn a_default_calendar_url_is_keyed_as_a_listing_keys_it() {
        let home = "/dav/cal/test%40pimalaya.org/";
        assert_eq!(
            collection_key(home, "/dav/cal/test%40pimalaya.org/default/").as_deref(),
            Some("default")
        );
        assert_eq!(collection_key("/cal", "/cal/work").as_deref(), Some("work"));
        assert_eq!(collection_key("/cal/", "/"), None);

        assert_eq!(
            collection_key("/cal/", "/cal/My%20Cal/").as_deref(),
            Some("My%20Cal")
        );
        assert_eq!(
            collection_key("/cal/", "/cal/R%C3%A9unions/").as_deref(),
            Some("R%C3%A9unions")
        );
    }

    /// A calendar outside the home set is keyed by its path, as io-webdav lists
    /// it.
    ///
    /// Google names every calendar `events` under its own path, so a last
    /// segment would make the default calendar any of them.
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
    ///
    /// A literal colon names it too; an escaped `/`, space or `%` stays
    /// encoded, decoding it naming another resource or none.
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
        assert_eq!(
            href_id("/dav/card/test/default/urn:uuid:4fbe8971-0bc3.vcf"),
            created
        );
        assert_eq!(href_id("/books/card%2D1%2Evcf"), "card-1.vcf");

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

    use crate::scripted::{Server, answer, reply};

    /// A calendar session over `replies`, its home set `/dav/cal/`.
    fn calendars(replies: Vec<String>) -> (DavClient, Server) {
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
        let requests = wire.sent();
        assert_eq!(wire.lines(), ["MOVE /dav/cal/tasks/t1.ics"]);
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

    /// A refused write is coded by its status and the precondition its body
    /// names.
    ///
    /// A 403 naming none, or `need-privileges`, is a matter of rights; one
    /// naming what the body failed is not.
    #[test]
    fn a_refused_write_is_coded_by_its_status_and_precondition() {
        let error = |body: &str| {
            format!(
                "<d:error xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\">{body}</d:error>"
            )
        };
        let xml = [("Content-Type", "application/xml")];
        let cases = [
            (
                "403 Forbidden",
                String::new(),
                Some(ErrorCode::PermissionDenied),
            ),
            (
                "403 Forbidden",
                error("<d:need-privileges/>"),
                Some(ErrorCode::PermissionDenied),
            ),
            ("403 Forbidden", error("<c:valid-calendar-data/>"), None),
            (
                "507 Insufficient Storage",
                String::new(),
                Some(ErrorCode::QuotaExceeded),
            ),
            (
                "403 Forbidden",
                error("<d:quota-not-exceeded/>"),
                Some(ErrorCode::QuotaExceeded),
            ),
            ("409 Conflict", String::new(), None),
        ];

        for (status, body, code) in cases {
            let (mut dav, _) = calendars(vec![answer(status, &xml, &body)]);
            let ical = "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n";
            let err = dav
                .update_item_stream("tasks", "t1.ics", ical.as_bytes(), Some("\"1\""))
                .unwrap_err();

            let failure = Failure::of(&err);
            assert_eq!(failure.code, code, "{status} {body}: {err:#}");
            assert_eq!(failure.error, "Cannot update t1.ics in tasks", "{err:#}");
        }
    }

    /// io-webdav moves no card: CardDAV answers none without a request.
    #[test]
    fn a_card_is_never_moved_server_side() {
        let (mut dav, wire) = DavClient::scripted(DavKind::Card, "/dav/card/", Vec::new());
        let moved = dav.move_item("a", "b", "c1.vcf", None).unwrap();
        assert!(moved.is_none());
        assert!(wire.sent().is_empty());
    }

    /// A relocation is a `MOVE`, never a create then a delete, which a
    /// server keeping one `UID` per account (Fastmail) refuses.
    #[test]
    fn a_relocation_moves_rather_than_recreates() {
        let (mut dav, wire) = calendars(vec![reply("201 Created", &[])]);

        dav.move_items("tasks", "work", &["t1.ics"]).unwrap();

        assert_eq!(wire.lines(), ["MOVE /dav/cal/tasks/t1.ics"]);
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
                "MOVE /dav/cal/tasks/t1.ics",
                "GET /dav/cal/tasks/t1.ics",
                "PUT /dav/cal/work/t1.ics",
                "DELETE /dav/cal/tasks/t1.ics",
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

    /// An id climbing out of its collection is refused before any request.
    ///
    /// A key or member id addressing the server root, the home set or a parent
    /// of either, as another source's store may hand over, sends nothing.
    #[test]
    fn an_id_climbing_out_of_its_collection_sends_nothing() {
        let accepted = || vec![reply("201 Created", &[]), reply("204 No Content", &[])];

        for key in ["..", "/", "", "a/../.."] {
            let (mut dav, wire) = calendars(accepted());
            assert!(dav.create_collection(key).is_err(), "{key:?}");
            assert!(dav.delete_collection(key).is_err(), "{key:?}");
            assert_eq!(wire.lines(), Vec::<String>::new(), "{key:?}");
        }

        for id in ["..", ".", ""] {
            let (mut dav, wire) = calendars(accepted());
            assert!(dav.delete_item("tasks", id, None).is_err(), "{id:?}");
            assert_eq!(wire.lines(), Vec::<String>::new(), "{id:?}");
        }
    }

    /// A delete answered 207 naming a member it could not delete (RFC 4918
    /// §9.6.1) is no delete done: the member is still there.
    #[test]
    fn a_delete_answered_with_a_failed_member_is_not_done() {
        let locked = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                      <d:multistatus xmlns:d=\"DAV:\"><d:response>\
                      <d:href>/dav/cal/tasks/t1.ics</d:href>\
                      <d:status>HTTP/1.1 423 Locked</d:status>\
                      </d:response></d:multistatus>";
        let (mut dav, _) = calendars(vec![answer("207 Multi-Status", &[], locked)]);

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

        let request = &wire.sent()[0];
        assert_eq!(wire.lines(), ["MOVE /caldav/v2/x%40g/events/t1.ics"]);
        assert!(
            carries(
                request,
                "destination: https://dav.example.org/caldav/v2/alice%40g/events/t1.ics"
            ),
            "{request}"
        );
    }

    /// A 207 answer carrying `responses`, the CalDAV namespace bound to `c`
    /// and the CardDAV one to `card`.
    fn multistatus(responses: &str) -> String {
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <d:multistatus xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\" \
             xmlns:card=\"urn:ietf:params:xml:ns:carddav\">{responses}</d:multistatus>"
        );
        answer(
            "207 Multi-Status",
            &[("Content-Type", "application/xml")],
            &xml,
        )
    }

    /// A response of `href` carrying the properties `props`.
    fn row(href: &str, props: &str) -> String {
        format!(
            "<d:response><d:href>{href}</d:href><d:propstat><d:prop>{props}</d:prop>\
             <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
        )
    }

    /// A response of `href` the server answers 404.
    fn missing(href: &str) -> String {
        format!(
            "<d:response><d:href>{href}</d:href>\
             <d:status>HTTP/1.1 404 Not Found</d:status></d:response>"
        )
    }

    /// A listed collection is keyed by the segment its server served.
    ///
    /// That is io-webdav 0.9.1's key (`My%20Cal`), never the decoded id (`My
    /// Cal`), a raw `My Cal` or `Réunions` kept too. Its reports are under its
    /// key: no `sync-collection`, so `PROPFIND` lists it.
    #[test]
    fn a_listed_collection_is_keyed_by_the_segment_served() {
        let calendar = "<d:resourcetype><d:collection/><c:calendar/></d:resourcetype>\
                        <d:supported-report-set><d:supported-report><d:report>\
                        <c:calendar-multiget/></d:report></d:supported-report>\
                        </d:supported-report-set>";
        let listing = [
            row("/dav/cal/My%20Cal/", calendar),
            row("/dav/cal/My Cal/", calendar),
            row("/dav/cal/R%C3%A9unions%202026/", calendar),
            row("https://dav.example.org/dav/cal/Réunions/", calendar),
            row("/team/x%20y/events/", calendar),
        ];
        let members = row(
            "/dav/cal/My%20Cal/r%C3%A9union%201.ics",
            "<d:getetag>\"e1\"</d:getetag>",
        );
        let (mut dav, wire) = calendars(vec![
            multistatus(&listing.concat()),
            multistatus(&members),
            multistatus(""),
        ]);

        let keys: Vec<String> = dav
            .list_collections(false)
            .unwrap()
            .into_iter()
            .map(|collection| collection.id)
            .collect();
        let listed = dav.enumerate("My%20Cal", None).unwrap();
        dav.enumerate("Réunions", None).unwrap();

        assert_eq!(
            keys,
            [
                "/team/x%20y/events",
                "My Cal",
                "My%20Cal",
                "R%C3%A9unions%202026",
                "Réunions"
            ]
        );
        assert_eq!(
            wire.lines()[1..],
            [
                "PROPFIND /dav/cal/My%20Cal/",
                "PROPFIND /dav/cal/R%C3%A9unions/"
            ]
        );
        let ids: Vec<&str> = listed.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["r%C3%A9union%201.ics"]);
    }

    /// A multiget answers each body, and each member gone, under [`href_id`].
    ///
    /// As the enumeration names members: an escape kept (`a%20b.ics`, which
    /// io-webdav decodes), a raw space or non-ASCII byte kept raw.
    #[test]
    fn a_multiget_answers_each_member_under_the_segment_served() {
        for (kind, home, data) in [
            (DavKind::Cal, "/dav/cal/", "c:calendar-data"),
            (DavKind::Card, "/dav/card/", "card:address-data"),
        ] {
            let extension = kind.extension();
            let body = format!("<d:getetag>\"e1\"</d:getetag><{data}>BEGIN</{data}>");
            let found = ["a%20b", "r%C3%A9union%201", "raw b", "réunion 2"]
                .map(|name| format!("{name}.{extension}"));
            let gone = ["gone%20%C3%A9", "gone é"].map(|name| format!("{name}.{extension}"));
            let rows: String = found
                .iter()
                .map(|id| row(&format!("{home}My%20Cal/{id}"), &body))
                .chain(
                    gone.iter()
                        .map(|id| missing(&format!("{home}My%20Cal/{id}"))),
                )
                .collect();
            let (mut dav, wire) = DavClient::scripted(kind, home, vec![multistatus(&rows)]);
            let ids: Vec<&str> = found.iter().chain(&gone).map(String::as_str).collect();
            let mut opened = Vec::new();
            let mut done = Vec::new();

            let answered = dav
                .fetch_bodies(
                    "My%20Cal",
                    &ids,
                    |id| {
                        opened.push(id.to_owned());
                        Ok(Vec::new())
                    },
                    |id, _, _| {
                        done.push(id.to_owned());
                        Ok(())
                    },
                )
                .unwrap();

            assert_eq!(opened, found, "{kind}");
            assert_eq!(done, opened, "{kind}");
            assert_eq!(answered, gone, "{kind}");
            let report = format!("REPORT {home}My%20Cal/");
            assert_eq!(wire.lines(), [report], "{kind}");
        }
    }

    /// A member the server names on create (a `Location`) keeps its segment.
    ///
    /// As io-webdav 0.9.1 read it: an escape kept (io-webdav decodes it), a raw
    /// space or non-ASCII byte kept raw, a query dropped.
    #[test]
    fn a_created_member_keeps_the_segment_its_location_served() {
        for (kind, home) in [(DavKind::Cal, "/dav/cal/"), (DavKind::Card, "/dav/card/")] {
            let extension = kind.extension();
            for (location, key) in [
                (
                    format!("{home}My%20Cal/r%C3%A9union%201.{extension}"),
                    "r%C3%A9union%201",
                ),
                (
                    format!("{home}My%20Cal/raw réunion.{extension}"),
                    "raw réunion",
                ),
                (
                    format!("https://dav.example.org{home}My%20Cal/x%20y.{extension}?v=1"),
                    "x%20y",
                ),
            ] {
                let created = reply("201 Created", &[("Location", &location)]);
                let (mut dav, wire) = DavClient::scripted(kind, home, vec![created]);

                let written = dav
                    .add_item_stream("My%20Cal", &b"BEGIN"[..], stated("t1"))
                    .unwrap();

                assert_eq!(written.id, format!("{key}.{extension}"), "{location}");
                let put = format!("PUT {home}My%20Cal/t1.{extension}");
                assert_eq!(wire.lines(), [put]);
            }
        }
    }

    /// A moved member keeps the id it was sent under, which io-webdav
    /// hands back decoded.
    #[test]
    fn a_moved_member_keeps_the_segment_it_was_sent_under() {
        let (mut dav, wire) = calendars(vec![reply("201 Created", &[])]);

        let moved = dav
            .move_item("My%20Cal", "R%C3%A9unions", "r%C3%A9union%201.ics", None)
            .unwrap()
            .expect("a CalDAV server moves");

        assert_eq!(moved.id, "r%C3%A9union%201.ics");
        assert_eq!(
            wire.lines(),
            ["MOVE /dav/cal/My%20Cal/r%C3%A9union%201.ics"]
        );
        let request = &wire.sent()[0];
        let destination = "destination: \
                           https://dav.example.org/dav/cal/R%C3%A9unions/r%C3%A9union%201.ics";
        assert!(carries(request, destination), "{request}");
    }

    /// A `sync-collection` answer listing `card`, truncated when
    /// `truncated`, carrying `token` when given.
    fn sync_answer(card: &str, truncated: bool, token: Option<&str>) -> Vec<u8> {
        let cut = match truncated {
            true => {
                "<d:response><d:href>/dav/books/default/</d:href>\
                 <d:status>HTTP/1.1 507 Insufficient Storage</d:status></d:response>"
            }
            false => "",
        };
        let token = token
            .map(|token| format!("<d:sync-token>{token}</d:sync-token>"))
            .unwrap_or_default();
        let xml = format!(
            "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\">\
             <d:response><d:href>/dav/books/default/{card}.vcf</d:href>\
             <d:propstat><d:prop><d:getetag>\"e-{card}\"</d:getetag></d:prop>\
             <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>\
             {cut}{token}</d:multistatus>"
        );
        format!(
            "HTTP/1.1 207 Multi-Status\r\nContent-Length: {}\r\n\r\n{xml}",
            xml.len()
        )
        .into_bytes()
    }

    /// A CardDAV session over `answers`, its discovery already made.
    fn replayed(answers: Vec<Vec<u8>>) -> DavClient {
        let server = Url::parse("https://dav.example.org/").unwrap();
        let inner = WebdavClientStd::from_parts(
            Server::replying(answers).open().unwrap(),
            WebdavAuth::None,
            server.clone(),
            None,
            None,
            Some(Url::parse("https://dav.example.org/dav/books/").unwrap()),
        );
        DavClient {
            kind: DavKind::Card,
            inner,
            server,
            tls: Tls::default(),
            auth: WebdavAuth::None,
            proxy: Proxy::None,
            throttle: Arc::new(Throttle::new("carddav")),
            tokens: BTreeMap::new(),
        }
    }

    /// A first listing the server keeps truncating is no complete snapshot:
    /// read as one, every card past the cut would be inferred deleted.
    #[test]
    fn a_listing_truncated_to_the_last_round_is_incomplete() {
        let answers = (0..MAX_SYNC_ROUNDS)
            .map(|round| sync_answer(&format!("c{round}"), true, Some(&format!("t{round}"))))
            .collect();
        let listed = replayed(answers).enumerate("default", None).unwrap();

        assert_eq!(listed.items.len(), MAX_SYNC_ROUNDS);
        assert!(!listed.complete);
        let last = format!("t{}", MAX_SYNC_ROUNDS - 1);
        assert_eq!(listed.checkpoint.as_deref(), Some(last.as_bytes()));
    }

    /// A truncated answer with no token ends the listing there, incomplete.
    ///
    /// Restarting would cut it at the same place, so the checkpoint stays the
    /// one it started from.
    #[test]
    fn a_truncated_answer_without_a_token_ends_the_listing() {
        let answers = (0..MAX_SYNC_ROUNDS)
            .map(|round| sync_answer(&format!("c{round}"), true, None))
            .collect();
        let listed = replayed(answers).enumerate("default", None).unwrap();

        let ids: Vec<_> = listed.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["c0.vcf"]);
        assert!(!listed.complete);
        assert_eq!(listed.checkpoint.as_deref(), Some(&b""[..]));

        let answers = vec![sync_answer("c0", true, None)];
        let listed = replayed(answers).enumerate("default", Some(b"t0")).unwrap();
        assert!(!listed.complete);
        assert_eq!(listed.checkpoint.as_deref(), Some(&b"t0"[..]));
    }

    /// A truncation followed to its end is a whole first listing.
    #[test]
    fn a_truncation_followed_to_its_end_is_complete() {
        let answers = vec![
            sync_answer("c0", true, Some("t0")),
            sync_answer("c1", false, Some("t1")),
        ];
        let listed = replayed(answers).enumerate("default", None).unwrap();

        assert_eq!(listed.items.len(), 2);
        assert!(listed.complete);
        assert_eq!(listed.checkpoint.as_deref(), Some(&b"t1"[..]));
    }

    /// A 207 answer listing `c0.vcf` and a `c1.vcf` row answered 403, through
    /// the `sync-collection` report when `token` is given, else `PROPFIND`.
    fn refused_answer(token: Option<&str>) -> Vec<u8> {
        let token = token
            .map(|token| format!("<d:sync-token>{token}</d:sync-token>"))
            .unwrap_or_default();
        let xml = format!(
            "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\">\
             <d:response><d:href>/dav/books/default/c0.vcf</d:href>\
             <d:propstat><d:prop><d:getetag>\"e-c0\"</d:getetag></d:prop>\
             <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>\
             <d:response><d:href>/dav/books/default/c1.vcf</d:href>\
             <d:status>HTTP/1.1 403 Forbidden</d:status></d:response>\
             {token}</d:multistatus>"
        );
        format!(
            "HTTP/1.1 207 Multi-Status\r\nContent-Length: {}\r\n\r\n{xml}",
            xml.len()
        )
        .into_bytes()
    }

    /// A member listed with an error makes the listing incomplete.
    ///
    /// Read as a whole collection, the member would read as deleted. The
    /// checkpoint stays the one the listing started from, so the next run asks
    /// again rather than skipping past the member's change.
    #[test]
    fn a_member_listed_with_an_error_makes_the_listing_incomplete() {
        let errored = vec![(String::from("c1.vcf"), Some(403))];

        let listed = replayed(vec![refused_answer(Some("t1"))])
            .enumerate("default", None)
            .unwrap();
        let ids: Vec<_> = listed.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["c0.vcf"]);
        assert!(!listed.complete);
        assert!(!listed.truncated);
        assert_eq!(listed.errored, errored);
        assert_eq!(listed.checkpoint.as_deref(), Some(&b""[..]));

        let listed = replayed(vec![refused_answer(Some("t1"))])
            .enumerate("default", Some(b"t0"))
            .unwrap();
        assert!(!listed.complete);
        assert_eq!(listed.errored, errored);
        assert_eq!(listed.checkpoint.as_deref(), Some(&b"t0"[..]));

        let listed = replayed(vec![refused_answer(None)])
            .list("default")
            .unwrap();
        assert_eq!(listed.items.len(), 1);
        assert!(!listed.complete);
        assert_eq!(listed.errored, errored);
    }

    /// An answer that is no whole multistatus fails the listing.
    ///
    /// A gateway's page, a cut document or an empty body, read as a listing of
    /// nothing, would state every member removed.
    #[test]
    fn an_answer_that_is_no_whole_multistatus_fails_the_listing() {
        let page = "<html><body>Sign in</body></html>";
        let cut = "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\">\
                   <d:response><d:href>/dav/books/default/c0.vcf</d:href>";

        for body in [page, cut, ""] {
            let answers = || {
                let answer = format!(
                    "HTTP/1.1 207 Multi-Status\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                vec![answer.into_bytes()]
            };

            let report = replayed(answers()).enumerate("default", None);
            assert!(report.is_err(), "{body:?}: {report:?}");
            let listing = replayed(answers()).list("default");
            assert!(listing.is_err(), "{body:?}: {listing:?}");
            let collections = replayed(answers()).list_collections(false);
            assert!(collections.is_err(), "{body:?}: {collections:?}");
        }
    }
}
