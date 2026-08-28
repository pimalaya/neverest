//! DAV client adapter for the shared cross-protocol client.
//!
//! [`DavClient`] wraps the std blocking io-webdav client behind the same
//! adapter surface as the IMAP and Graph backends, for CardDAV (RFC 6352) and
//! CalDAV (RFC 4791) alike. It is the **mutable-content** side of the sync: a
//! card or an event is edited in place rather than replaced, so this is where
//! the revision plumbing (ETags, `If-Match`) built blind against mail finally
//! has a server that exercises it.
//!
//! One adapter serves both because the two protocols differ in three things
//! this file names ([`DavKind`]) and in nothing else: the home set they
//! discover, the collection they list, and the extension a new resource is
//! named with. Everything below that (enumeration, multiget, conditional
//! writes) is RFC 4918 and RFC 6578, which both sit on.
//!
//! Three shapes differ from the mail backends:
//!
//! - **Collections are address books and calendars**, keyed by their path
//!   segment id rather than their display name. A display name is optional,
//!   may collide between two collections and may change under a running sync,
//!   none of which a collection key survives.
//! - **Items resolve at `Full` only.** A `sync-collection` REPORT returns
//!   hrefs and ETags, never a `UID`, so there is no cheap `Meta` tier and
//!   [`crate::kind::Kind::parse_summary`] is `None` for both DAV kinds.
//! - **Flags are known-empty, not unknown.** DAV has no flag concept, so
//!   every entry reports an empty set, which the engine reads as "no flags"
//!   rather than "not fetched".
//!
//! Enumeration is RFC 6578 where the server implements it: an initial report
//! (no token) returns the whole member set plus the token, and a later one
//! returns only what changed. A server that rejects the stored token
//! ([`WebdavSyncCollectionError::InvalidSyncToken`]) is answered with a fresh full
//! report, which the engine reads as a complete snapshot and reconciles
//! against, exactly as an expired Graph delta link or an IMAP UIDVALIDITY
//! bump would be. A truncated report (RFC 6578 §3.6) is drained by running it
//! again from the token it returned.
//!
//! `sync-collection` is an extension, though, and a deployment may implement
//! none of it: its `supported-report-set` then holds the multiget and query
//! reports alone, and the REPORT comes back with the RFC 3253 §3.6
//! `DAV:supported-report` precondition. Such a collection is listed with a
//! `PROPFIND` instead, which yields the same ids and ETags with no token, so it
//! enumerates in full on every run rather than not syncing at all. The listing
//! is chosen from the advertised report set where a run already listed the
//! collections, and from the refusal otherwise.

use std::{
    collections::BTreeSet,
    fmt,
    io::{ErrorKind, Read, Write},
};

use anyhow::{Context, Result, bail};
use io_http::rfc9112::send::Http11SendError;
use io_webdav::{
    client::{WebdavClientStd, WebdavClientStdError},
    rfc4791::calendar::CaldavCalendar,
    rfc4918::{WebdavAuth, follow_redirects::WebdavFollowRedirectsError, send::WebdavSendError},
    rfc6352::addressbook::CarddavAddressbook,
    rfc6578::sync_collection::{
        SYNC_COLLECTION, WebdavSyncChange, WebdavSyncCollectionError, WebdavSyncCollectionOptions,
        WebdavSyncDelta,
    },
};
use log::{debug, warn};
use pimalaya_stream::tls::Tls;
use url::Url;

use crate::{
    client::{EnumEntry, Enumeration, WrittenItem},
    item::{collection::Collection, flag::Flag, flag::FlagOp},
    kind::{Kind, LinkId},
};

/// How many truncated rounds a single enumeration drains before giving up,
/// so a server answering "truncated" forever cannot spin the sync.
const MAX_SYNC_ROUNDS: usize = 32;

/// Which DAV flavour a session speaks.
///
/// The whole of the protocol difference: everything else the adapter does is
/// the WebDAV both sit on, so the sync sees one backend with two media types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DavKind {
    /// CardDAV (RFC 6352): address books of contact cards.
    Card,
    /// CalDAV (RFC 4791): calendars of calendar object resources.
    Cal,
}

impl DavKind {
    /// The IANA media type of the resources this flavour syncs, recorded as
    /// the pimdir collection's `kind`.
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

    /// The configuration table this flavour is written under, which is also
    /// the id of the source the direct-backend sugar builds from it.
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
    /// The connect arguments, kept so a connection the server closed can be
    /// reopened (see [`op`](DavClient::op)).
    server: Url,
    tls: Tls,
    auth: WebdavAuth,
}

impl DavClient {
    /// Opens the session and discovers the home set, so a misconfigured URL or
    /// credential fails here rather than at the first enumeration.
    pub fn connect(kind: DavKind, server: &Url, tls: &Tls, auth: WebdavAuth) -> Result<Self> {
        let inner = WebdavClientStd::connect(server, tls, auth.clone())
            .with_context(|| format!("Cannot connect to the {kind} server"))?;
        let mut client = Self {
            kind,
            inner,
            server: server.clone(),
            tls: tls.clone(),
            auth,
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

    /// Runs one WebDAV exchange, reopening the connection and running it
    /// again when the server had closed it.
    ///
    /// io-webdav holds a single stream and reports no keep-alive hint, so a
    /// server that answers HTTP/1.0 (Radicale's built-in server) or sends
    /// `Connection: close` leaves the next request written into a socket the
    /// peer already hung up on, and everything after the first exchange
    /// fails. The Graph backend reconnects on the hint its client does
    /// report; this is the same repair without one.
    ///
    /// Only an end-of-stream failure is retried, which is the shape of a
    /// request the server never read, so a create or a delete is not
    /// replayed against a server that acted on it. The reopened client keeps
    /// the discovered principal and home-set URLs, so a reconnect costs a
    /// handshake and no discovery.
    fn op<T>(
        &mut self,
        mut run: impl FnMut(&mut WebdavClientStd) -> Result<T, WebdavClientStdError>,
    ) -> Result<T, WebdavClientStdError> {
        match run(&mut self.inner) {
            Err(err) if is_connection_closed(&err) => {
                debug!("{} connection closed by the server, reopening", self.kind);
                self.reconnect()?;
                run(&mut self.inner)
            }
            out => out,
        }
    }

    /// Reopens the connection, carrying the discovery this session already
    /// paid for over to the new client.
    ///
    /// Both home sets are carried without asking which one this session
    /// discovered: only its own is ever populated, so the other copies an
    /// absence.
    fn reconnect(&mut self) -> Result<(), WebdavClientStdError> {
        let mut inner = WebdavClientStd::connect(&self.server, &self.tls, self.auth.clone())?;
        inner.principal_url = self.inner.principal_url.clone();
        inner.addressbook_home_set = self.inner.addressbook_home_set.clone();
        inner.addressbook_reports = self.inner.addressbook_reports.clone();
        inner.calendar_home_set = self.inner.calendar_home_set.clone();
        inner.calendar_reports = self.inner.calendar_reports.clone();
        self.inner = inner;

        Ok(())
    }

    /// Lists every collection of this session's kind. Counts are never
    /// reported: DAV has no cheap total, and paying a full enumeration per
    /// collection to render one number is not a trade this makes.
    pub fn list_collections(&mut self, _with_counts: bool) -> Result<Vec<Collection>> {
        let kind = self.kind;
        let ids: BTreeSet<String> = match kind {
            DavKind::Card => self
                .op(WebdavClientStd::list_addressbooks)
                .map(|books| books.into_iter().map(|book| book.id).collect()),
            DavKind::Cal => self
                .op(WebdavClientStd::list_calendars)
                .map(|calendars| calendars.into_iter().map(|calendar| calendar.id).collect()),
        }
        .with_context(|| format!("Cannot list the {kind} collections"))?;

        // Both the id and the name are the path segment: a collection key must
        // be stable and unique, and a DAV display name is neither (optional,
        // mutable, free to collide).
        Ok(ids
            .into_iter()
            .map(|id| Collection {
                name: id.clone(),
                id,
                total: None,
                unread: None,
            })
            .collect())
    }

    /// Creates a collection, named after the collection key.
    pub fn create_collection(&mut self, collection: &str) -> Result<()> {
        let kind = self.kind;
        let name = Some(collection.to_owned());

        match kind {
            DavKind::Card => {
                let book = CarddavAddressbook {
                    id: collection.to_owned(),
                    display_name: name,
                    ..Default::default()
                };
                self.op(|dav| dav.create_addressbook(&book))
            }
            DavKind::Cal => {
                let calendar = CaldavCalendar {
                    id: collection.to_owned(),
                    display_name: name,
                    ..Default::default()
                };
                self.op(|dav| dav.create_calendar(&calendar))
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

    /// Enumerates a collection through `sync-collection`, carrying the
    /// server's sync token as the engine's opaque checkpoint, and listing it
    /// instead on a server that does not implement the report.
    ///
    /// RFC 6578 is an extension: a deployment may advertise a
    /// `supported-report-set` holding the multiget and query reports and no
    /// `sync-collection`, then answer the REPORT with the RFC 3253 §3.6
    /// `DAV:supported-report` precondition. Without the fallback that is a hard
    /// enumerate failure, so every collection on such a server is unsyncable.
    ///
    /// The choice is made twice over: from the `supported-report-set` io-webdav
    /// caches while listing, which a sync run always pays for first, and from
    /// the refusal itself, for a server that advertises one thing and answers
    /// another.
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

        // The fallback keeps no token, so its checkpoint is empty, which means
        // the same as no cursor at all.
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

    /// One full listing through a `PROPFIND` at Depth 1, for a server with no
    /// `sync-collection`. It carries no token, so the checkpoint is empty and
    /// every run lists the whole collection: correct, and the price of a
    /// server offering nothing incremental.
    ///
    /// A `PROPFIND` rather than the query report, which is the other thing such
    /// a server advertises: a query carries a filter the server evaluates by
    /// parsing every resource, so one resource it cannot parse fails the whole
    /// enumeration, where a `PROPFIND` reads names and ETags out of the store
    /// and lists the collection past it.
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
            // A truncated listing holds part of the collection, and a partial
            // snapshot read as a complete one deletes every member the server
            // left out.
            complete: !delta.truncated,
            checkpoint: Vec::new(),
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
            checkpoint: token.unwrap_or_default().into_bytes(),
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

    /// Creates a resource, addressing it by the item's `UID` so the href a
    /// server assigns stays derivable from the body itself, plus the minted
    /// part of its key where it has one, so a second copy of that `UID` never
    /// lands on the href its twin holds.
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

    /// Replaces a resource in place, conditionally on the last-synced ETag: a
    /// server whose copy moved since rejects the write rather than losing the
    /// other edit, which is what the engine's conflict path is waiting for.
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
    pub fn delete_item(
        &mut self,
        collection: &str,
        id: &str,
        if_match: Option<&str>,
    ) -> Result<()> {
        self.delete(collection, id, if_match)
            .with_context(|| format!("Cannot delete {id} from {collection}"))
    }

    /// Moves resources between collections. DAV has no server-side move here,
    /// so each is re-created at the target and deleted from the source; the
    /// delete only runs once the create was accepted, so a failure leaves the
    /// resource where it was rather than nowhere.
    pub fn move_items(&mut self, from: &str, to: &str, ids: &[&str]) -> Result<()> {
        for id in ids {
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

    /// Rejected: DAV has no flags, and the enumeration reports every member
    /// as known-empty, so the engine never derives a flag change to push. A
    /// call here means something upstream invented one.
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
                .op(|dav| dav.create_card(collection, id, body.clone()))
                .map(|created| WrittenItem {
                    id: created.id,
                    revision: created.etag,
                }),
            DavKind::Cal => self
                .op(|dav| dav.create_item(collection, id, body.clone()))
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

/// One changed member of a `sync-collection` delta as an enumeration entry.
fn entry(change: WebdavSyncChange) -> EnumEntry {
    EnumEntry {
        id: href_id(&change.href),
        flags: BTreeSet::new(),
        revision: change.etag,
    }
}

/// The addressing id of a member href: its last non-empty path segment,
/// exactly as io-webdav addresses resources.
fn href_id(href: &str) -> String {
    href.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(href)
        .to_owned()
}

/// The resource name a new item is created under: its `UID` plus the kind's
/// conventional extension, and the minted part of its key where the collection
/// it came from held that `UID` twice.
///
/// The body fallback is only for an item whose key states no identity at all.
/// A minted key states one its twin already took, so a name derived from the
/// body would collide by construction, and a colliding `PUT` is not refused by
/// a server but applied to the resource already there: the copy that synced
/// first would be overwritten by the copy being appended, losing it and
/// reporting success.
fn resource_id(kind: DavKind, link: LinkId<'_>, body: &[u8]) -> String {
    let extension = kind.extension();
    let hint = link.hint.map(str::trim).filter(|hint| !hint.is_empty());

    let name = match (hint, link.mint) {
        (Some(uid), None) => sanitize(uid),
        (Some(uid), Some(mint)) => format!("{}-{}", sanitize(uid), sanitize(mint)),
        (None, Some(mint)) => sanitize(mint),
        (None, None) => {
            let (link, _, _) = kind.item_kind().parse_body(body, body.len() as u64);
            sanitize(link.0.trim_start_matches("hash:"))
        }
    };

    format!("{name}.{extension}")
}

/// Keeps a `UID` addressable as one path segment: a `UID` is free-form text
/// and may legally carry a `/` or a space, neither of which survives being
/// spliced into a URL path.
fn sanitize(uid: &str) -> String {
    uid.chars()
        .map(|char| match char {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | ':' => char,
            _ => '-',
        })
        .collect()
}

/// Whether the exchange died on a connection the server had already closed,
/// the one failure [`DavClient::op`] repairs by reopening it.
///
/// An end of stream where a response was due says the request was never
/// answered, and a broken pipe or a reset says it was never written, so
/// neither leaves a half-applied write behind. Every other failure is the
/// server's answer and is reported as it is.
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

/// Whether a failed write was refused because the collection already holds a
/// resource carrying that `UID`: the `no-uid-conflict` precondition of RFC
/// 4791 §5.3.2 and RFC 6352 §6.3.2, which io-webdav names.
///
/// The error crosses the client seam as an [`anyhow::Error`], so the typed
/// refusal is read back out of its chain. This is the seam where a DAV status
/// becomes something the report can say in the user's terms, and the only
/// caller is the push path in [`crate::offline::remote`], a build without the
/// `dav` feature having no write that can be refused this way.
pub fn is_duplicate_uid(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<WebdavClientStdError>()
            .is_some_and(WebdavClientStdError::is_duplicate_uid)
    })
}

/// Whether the server advertises `sync-collection` for a collection.
///
/// A collection nobody listed counts as supporting it: the report is what
/// enumerates, and a server that does not implement it names the refusal
/// itself, so the unknown case costs one failed REPORT and never a wrong
/// enumeration.
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
mod tests {
    use super::*;

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
            "urn:uuid:4fbe8971-0bc3.vcf"
        );
        assert_eq!(
            resource_id(DavKind::Card, stated("a b/c"), b""),
            "a-b-c.vcf"
        );
    }

    /// Two items sharing a `UID` reach two hrefs. The second one's key was
    /// minted on the href it was read from, and that part reaching the name is
    /// what keeps the `PUT` off the resource the first copy already holds.
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

    /// A pair carrying no `UID` at all is minted over the kind's digest
    /// fallback, and the body must not name either copy: two bodies that hash
    /// the same are what got them minted, so a body-derived name is the
    /// collision itself.
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
    /// `sync-collection` is one to list instead, decided before a REPORT is
    /// sent. Which reports a server advertises is io-webdav's reading; which
    /// enumeration that buys is this crate's decision.
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
}
