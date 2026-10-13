//! # Microsoft Graph client
//!
//! [`GraphClient`] wraps the std blocking io-msgraph client behind the same
//! adapter surface as the IMAP backend, with the folder name map and the
//! connection it watches and reopens ([`wire`]): never reused past a minute
//! idle or after an exchange it did not read whole, a read sent again once
//! when a reused connection was found dead before answering.
//!
//! Bodies come in JSON batches bounded in octets by the sizes the listings
//! state ([`SizeHints`]), each committed as soon as it is decoded. A flag
//! set, a move or a delete on an item gone from the server counts as done;
//! a copy of it stays an error.
//!
//! `enumerate` lists mail as [`mail`] describes: a folder-wide message delta
//! link as the engine's opaque checkpoint (HTTP 410 means an expired link and
//! restarts a round), the mail of a scope listed by its `sentDateTime`.
//! Folders are listed two levels deep, named `Parent/Child`; deeper nesting
//! is not replicated.
//!
//! Push scope is honest: flag changes, deletes, moves and copies push, an
//! append lands in Drafts only (Graph creates every MIME message as a draft)
//! and mailbox mutations are rejected.
//!
//! Graph message ids are mutable across folder moves (no immutable-id support,
//! which would change the ids stores already bind), so a moved message is
//! listed by its new folder under a new id. A copy reports that id; a move
//! leaves it to the target's next enumeration, whose fetch lands the pending
//! create by its `Message-ID`, as after an IMAP `MOVE`. A delta reset never
//! changes handle identity, so no handle-space rebuild follows, unlike the
//! IMAP UIDVALIDITY path.
//!
//! One client serves one [`GraphKind`], the way one DAV adapter serves
//! CardDAV and CalDAV: a mail session speaks folders and messages, a
//! contacts session ([`contacts`]) contact folders and vCards.

mod calendar;
mod contacts;
mod mail;
mod wire;

use std::{
    collections::{BTreeMap, HashMap},
    io::{Read, Write},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, bail};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD},
};
use chrono::{DateTime, FixedOffset};
use io_msgraph::v1::{
    client::{
        MsgraphClientStd, MsgraphClientStdConnectOptions, MsgraphClientStdError, MsgraphStream,
    },
    rest::batch::{MSGRAPH_BATCH_MAX_REQUESTS, MsgraphBatchRequest, MsgraphBatchResponses},
    rest::users::{
        mail_folders::{
            MsgraphMailFolder,
            list::{MsgraphMailFoldersListParams, MsgraphMailFoldersListResponse},
        },
        messages::{MsgraphFlagStatus, MsgraphFollowupFlag, MsgraphMessage},
    },
    send::{MSGRAPH_API_BASE, MsgraphSend, MsgraphSendError, MsgraphSendOutput, user_path},
};
use io_pimdir::remote::PimdirEnumerate;
use log::{debug, trace, warn};
use pimalaya_stream::{
    proxy::Proxy,
    stream::{Stream, TlsConnectOptions},
    tls::Tls,
};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use self::wire::{Connect, Exchange, Shared, Watched};
use crate::{
    client::{Gone, Held, Listed, StoredSize, WrittenItem},
    item::{
        collection::Collection,
        flag::{Flag, FlagOp, IanaFlag},
        summary::{ItemSummary, normalize_message_id},
    },
    throttle::{self, Request, Throttle},
};

/// The page size requested when listing mail folders.
const FOLDER_PAGE_SIZE: u32 = 100;

/// Graph's well-known mail folders, with the role each states.
const WELL_KNOWN_FOLDERS: [(&str, &str); 6] = [
    ("inbox", "inbox"),
    ("sentitems", "sent"),
    ("drafts", "drafts"),
    ("deleteditems", "trash"),
    ("junkemail", "junk"),
    ("archive", "archive"),
];

/// The domain a Graph session syncs, Graph carrying several behind one
/// host and one credential.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphKind {
    /// Mail folders and messages, as `message/rfc822`.
    Mail,
    /// Contact folders and contacts, as `text/vcard`.
    Contacts,
    /// Calendars and events, as `text/calendar`.
    Calendar,
}

impl GraphKind {
    /// The IANA media type of the items this kind syncs.
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Mail => "message/rfc822",
            Self::Contacts => "text/vcard",
            Self::Calendar => "text/calendar",
        }
    }
}

/// The live Microsoft Graph session of one side.
pub struct GraphClient {
    /// The domain this session syncs.
    kind: GraphKind,
    inner: MsgraphClientStd,
    /// Opens a new connection to the Graph API, for reopens.
    connect: Connect,
    /// What the exchange in flight did on the stream ([`wire`]).
    exchange: Shared,
    /// Folder display name (or `Parent/Child` path) to folder id, refreshed
    /// from the folder listing when a name misses.
    folders: HashMap<String, String>,
    /// What the run keeps of each mail folder between its pages.
    kept: HashMap<String, mail::Folder>,
    /// The sizes the listings stated, shared by the source's connections,
    /// which plan the body batches by.
    sizes: SizeHints,
    /// Whether a whole-folder delta asks for the size `$expand`, until
    /// Graph refuses it once.
    delta_expands: bool,
    /// The id of the well-known Drafts folder, resolved on the first append.
    drafts: Option<String>,
    /// Whether the stream may carry the next exchange: the last one read a
    /// whole answer and the server kept the connection. When false the next
    /// operation reopens it.
    alive: bool,
    /// Whether the stream carried an exchange already, so the server may
    /// have closed it while it idled.
    reused: bool,
    /// When the stream last carried an exchange.
    used: Instant,
    /// The source's back-off, shared by its connections.
    throttle: Arc<Throttle>,
}

impl GraphClient {
    /// Opens the TLS connection to the Graph API with the given bearer token,
    /// scoped to the `user` mailbox owner (`me` or a user id), for one kind.
    /// `sizes` is shared by every connection of the source.
    pub fn connect(
        kind: GraphKind,
        token: &SecretString,
        user: &str,
        tls: Tls,
        sizes: SizeHints,
        throttle: Arc<Throttle>,
    ) -> Result<Self> {
        let connect: Connect = Box::new(move || {
            let url =
                Url::parse(MSGRAPH_API_BASE).context("Cannot parse the Graph API base URL")?;
            let host = url.host_str().context("Graph API base URL has no host")?;
            let opts = TlsConnectOptions {
                tls: tls.clone(),
                proxy: Proxy::None,
                ..Default::default()
            };
            let stream = Stream::connect_tls(host, url.port().unwrap_or(443), opts)?;
            stream.set_read_timeout(Some(wire::READ_TIMEOUT))?;
            Ok(Box::new(stream) as Box<dyn MsgraphStream>)
        });

        Self::over(kind, token, user, connect, sizes, throttle)
            .context("Cannot connect to Microsoft Graph")
    }

    /// A session over the connections `connect` opens, the first one at once.
    fn over(
        kind: GraphKind,
        token: &SecretString,
        user: &str,
        mut connect: Connect,
        sizes: SizeHints,
        throttle: Arc<Throttle>,
    ) -> Result<Self> {
        let exchange = Shared::default();
        let stream = Watched::new(connect()?, Arc::clone(&exchange));
        let options = MsgraphClientStdConnectOptions {
            user_id: user.to_owned(),
            ..Default::default()
        };
        let inner = MsgraphClientStd::new(stream, token.expose_secret(), options);

        Ok(Self {
            kind,
            inner,
            connect,
            exchange,
            folders: HashMap::new(),
            kept: HashMap::new(),
            sizes,
            delta_expands: true,
            drafts: None,
            alive: true,
            reused: false,
            used: Instant::now(),
            throttle,
        })
    }

    /// Reopens the stream to the Graph API endpoint, keeping the credential.
    fn reconnect(&mut self) -> Result<(), MsgraphClientStdError> {
        debug!("reopening the graph stream");

        let stream = (self.connect)().map_err(MsgraphClientStdError::Tls)?;
        self.inner
            .set_stream(Watched::new(stream, Arc::clone(&self.exchange)));
        self.alive = true;
        self.reused = false;
        Ok(())
    }

    /// Runs one Graph operation on a connection fit for it
    /// ([`exchange`](Self::exchange)).
    ///
    /// The source's [`Throttle`] sends it again while Graph throttles it
    /// (429, 503), backing off: io-msgraph keeps no `Retry-After` but in a
    /// batch's answers. A request creating something goes through
    /// [`create`](Self::create) instead.
    fn op<T>(
        &mut self,
        run: impl FnMut(&mut MsgraphClientStd) -> Result<MsgraphSendOutput<T>, MsgraphClientStdError>,
    ) -> Result<T, MsgraphClientStdError> {
        self.send(Request::Idempotent, false, run)
    }

    /// Runs one Graph request that creates or sends something: sent again on
    /// a 429 alone, which Graph answers before doing anything, never on a
    /// 503, after which the message, the copy or the event may exist.
    fn create<T>(
        &mut self,
        run: impl FnMut(&mut MsgraphClientStd) -> Result<MsgraphSendOutput<T>, MsgraphClientStdError>,
    ) -> Result<T, MsgraphClientStdError> {
        self.send(Request::Create, false, run)
    }

    /// Runs one Graph request of `request`, as [`op`](Self::op) describes.
    /// `reads` vouches that a request other than a `GET` reads only (a
    /// `$batch` of gets), so it may be sent again on a dead connection.
    fn send<T>(
        &mut self,
        request: Request,
        reads: bool,
        mut run: impl FnMut(
            &mut MsgraphClientStd,
        ) -> Result<MsgraphSendOutput<T>, MsgraphClientStdError>,
    ) -> Result<T, MsgraphClientStdError> {
        let throttle = Arc::clone(&self.throttle);
        throttle.call(
            1,
            || self.exchange(reads, &mut run),
            |err| match err {
                MsgraphClientStdError::Send(send)
                    if send.status().is_some_and(|status| request.retries(status)) =>
                {
                    Some(None)
                }
                _ => None,
            },
            |message| {
                MsgraphClientStdError::Send(MsgraphSendError::Api {
                    status: 429,
                    code: String::from("throttled"),
                    message,
                })
            },
        )
    }

    /// Runs one exchange on a connection fit for it ([`wire`]): one the last
    /// exchange left unfit (a failure short of a whole answer, a connection
    /// the server ended or asked to close), or that idled past
    /// [`wire::IDLE`], is reopened first. A request that only reads, met
    /// by a reused connection already dead, is sent again once on a new
    /// one; never one that timed out, nor one that writes.
    fn exchange<T>(
        &mut self,
        reads: bool,
        run: &mut impl FnMut(
            &mut MsgraphClientStd,
        ) -> Result<MsgraphSendOutput<T>, MsgraphClientStdError>,
    ) -> Result<T, MsgraphClientStdError> {
        // NOTE: cleared before a reopen too, so a reopen that fails leaves
        // no earlier attempt's record behind.
        wire::begin(&self.exchange);
        if self.alive && self.reused && self.used.elapsed() > wire::IDLE {
            debug!(
                "graph stream idle for {}s, not reused",
                self.used.elapsed().as_secs()
            );
            self.alive = false;
        }
        if !self.alive {
            self.reconnect()?;
        }

        let reused = self.reused;
        let err = match self.attempt(run) {
            Ok(out) => return Ok(out),
            Err(err) => err,
        };

        let (reading, died) = wire::read(&self.exchange, |exchange| {
            (reads || exchange.reads(), wire::died(&err, exchange))
        });
        if !(reused && reading && died) {
            if wire::timed_out(&err) {
                debug!("graph answer timed out, the request is not sent again");
            }
            return Err(err);
        }

        debug!("graph connection closed while it idled ({err}), sending again on a new one");
        self.reconnect()?;
        self.attempt(run)
    }

    /// Runs `run` once on the current stream and settles what it left of
    /// the connection: fit for the next exchange only after a whole answer
    /// on a stream the server neither ended nor asked to close.
    fn attempt<T>(
        &mut self,
        run: &mut impl FnMut(
            &mut MsgraphClientStd,
        ) -> Result<MsgraphSendOutput<T>, MsgraphClientStdError>,
    ) -> Result<T, MsgraphClientStdError> {
        wire::begin(&self.exchange);
        self.reused = true;
        let out = run(&mut self.inner);
        self.used = Instant::now();

        let ended = wire::read(&self.exchange, Exchange::ended);
        match out {
            Ok(out) => {
                self.alive = out.keep_alive && !ended;
                Ok(out.response)
            }
            Err(err) => {
                self.alive = wire::answered(&err) && !ended;
                Err(err)
            }
        }
    }

    /// The domain this session syncs.
    pub fn kind(&self) -> GraphKind {
        self.kind
    }

    /// Lists the session's collections: mail folders or contact folders.
    pub fn list_collections(&mut self, with_counts: bool) -> Result<Vec<Collection>> {
        match self.kind {
            GraphKind::Mail => self.list_mailboxes(with_counts),
            GraphKind::Contacts => self.list_contact_folders(),
            GraphKind::Calendar => self.list_calendars(),
        }
    }

    /// Lists one page of a collection: a page of a mail round or a mail
    /// delta ([`mail`]), or a whole contacts delta or calendar listing.
    /// `held` is what the store binds of a mail folder.
    pub fn enumerate(
        &mut self,
        collection: &str,
        request: &PimdirEnumerate,
        held: Held<'_>,
    ) -> Result<Listed> {
        let checkpoint = request
            .checkpoint()
            .map(|checkpoint| checkpoint.0.as_slice());
        match self.kind {
            GraphKind::Mail => {
                // NOTE: what the run keeps of the folder is taken out for
                // the listing, which borrows the session as its wire.
                let mut folder = self.kept.remove(collection).unwrap_or_default();
                let listed = mail::enumerate(self, &mut folder, collection, request, held);
                self.sizes.note(folder.sizes.drain(..));
                self.kept.insert(collection.to_owned(), folder);
                listed
            }
            GraphKind::Contacts => self
                .enumerate_contacts(collection, checkpoint)
                .map(Listed::Page),
            GraphKind::Calendar => self.enumerate_calendar(collection).map(Listed::Page),
        }
    }

    /// Fetches summaries for an id set; contacts resolve at `Full` instead.
    pub fn fetch_summaries(&mut self, collection: &str, ids: &[&str]) -> Result<Vec<ItemSummary>> {
        match self.kind {
            GraphKind::Mail => self.fetch_envelopes(collection, ids),
            GraphKind::Contacts => {
                bail!("Graph contacts have no summary tier (they resolve at Full)")
            }
            GraphKind::Calendar => {
                bail!("Graph events have no summary tier (they resolve at Full)")
            }
        }
    }

    /// Streams the bodies of an id set into a sink `open`ed and `done` per
    /// item, `done` receiving the revision on contacts, and answers the ids
    /// Graph answered 404, gone since they were listed. `sizes` is what the
    /// store records of each message.
    pub fn fetch_bodies<S: Write>(
        &mut self,
        collection: &str,
        ids: &[&str],
        sizes: StoredSize<'_>,
        open: impl FnMut(&str) -> std::io::Result<S>,
        done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<Vec<String>> {
        match self.kind {
            GraphKind::Mail => self.fetch_messages(collection, ids, sizes, open, done),
            GraphKind::Contacts => self.fetch_contacts(ids, open, done),
            GraphKind::Calendar => self.fetch_events(ids, open, done),
        }
    }

    /// Streams one item into `sink`, returning its revision on contacts;
    /// one Graph answers 404 fails with [`Gone`].
    pub fn get_item_stream(
        &mut self,
        collection: &str,
        id: &str,
        sink: impl Write,
    ) -> Result<Option<String>> {
        let read = match self.kind {
            GraphKind::Mail => self.get_message_stream(collection, id, sink).map(|()| None),
            GraphKind::Contacts => self.get_contact_stream(id, sink),
            GraphKind::Calendar => self.get_event_stream(id, sink),
        };
        read.map_err(|err| match chain_not_found(&err) {
            true => err.context(Gone(id.to_owned())),
            false => err,
        })
    }

    /// Creates an item; a message lands in Drafts only, with `flags`.
    pub fn add_item_stream(
        &mut self,
        collection: &str,
        flags: &[Flag],
        source: impl Read,
    ) -> Result<WrittenItem> {
        match self.kind {
            GraphKind::Mail => self.add_message(collection, flags, source),
            GraphKind::Contacts => self.add_contact(collection, source),
            GraphKind::Calendar => self.add_event(collection, source),
        }
    }

    /// Replaces an item in place on `if_match`; mail bodies are immutable.
    pub fn update_item_stream(
        &mut self,
        id: &str,
        source: impl Read,
        if_match: Option<&str>,
    ) -> Result<Option<String>> {
        match self.kind {
            GraphKind::Mail => bail!("Graph message bodies are immutable (in-place update)"),
            GraphKind::Contacts => self.update_contact(id, source, if_match),
            GraphKind::Calendar => self.update_event(id, source, if_match),
        }
    }

    /// Deletes one item, conditionally on `if_match` for contacts.
    pub fn delete_item(&mut self, id: &str, if_match: Option<&str>) -> Result<()> {
        match self.kind {
            GraphKind::Mail => self.delete_message(id),
            GraphKind::Contacts => self.delete_contact(id, if_match),
            GraphKind::Calendar => self.delete_event(id, if_match),
        }
    }

    /// Lists the synced mail folders as shared mailboxes, every top-level
    /// folder plus one level of children named `Parent/Child`.
    ///
    /// The name map is refreshed as a side effect; counts are not populated.
    fn list_mailboxes(&mut self, _with_counts: bool) -> Result<Vec<Collection>> {
        Ok(self
            .list_folders()?
            .into_iter()
            .map(|(name, _)| Collection {
                id: name.clone(),
                name,
                total: None,
                unread: None,
                role: None,
            })
            .collect())
    }

    /// The collection a new item goes to when none is named: the default
    /// calendar (`isDefaultCalendar`), the default Contacts folder; none for
    /// mail, whose folders have roles instead. A check-only probe.
    pub fn default_collection(&mut self) -> Result<Option<String>> {
        match self.kind {
            GraphKind::Mail => Ok(None),
            GraphKind::Contacts => Ok(Some(contacts::CONTACTS_FOLDER.to_owned())),
            GraphKind::Calendar => self.default_calendar(),
        }
    }

    /// The role each mail folder states, by folder name: one well-known
    /// folder lookup per role, the answered id matched to the listing. A
    /// well-known folder the mailbox does not have (often `archive`) is
    /// skipped. A check-only probe; no role on contacts or calendars.
    pub fn mailbox_roles(&mut self) -> Result<BTreeMap<String, String>> {
        if self.kind != GraphKind::Mail {
            return Ok(BTreeMap::new());
        }

        let folders = self.list_folders()?;
        let mut stated = Vec::new();

        for (well_known, role) in WELL_KNOWN_FOLDERS {
            match self.op(|client| client.mail_folder_get(well_known)) {
                Ok(folder) => stated.push((folder.id, role)),
                Err(err) => debug!("no well-known {well_known} folder: {err}"),
            }
        }

        Ok(roles_by_id(&folders, &stated))
    }

    /// Lists the replicated folder names with their ids, refreshing the name
    /// map as a side effect.
    fn list_folders(&mut self) -> Result<Vec<(String, String)>> {
        debug!("begin graph folder listing");

        let params = MsgraphMailFoldersListParams {
            top: Some(FOLDER_PAGE_SIZE),
            ..Default::default()
        };
        let mut entries = Vec::new();
        let mut parents = Vec::new();

        let mut page = self
            .op(|client| client.mail_folders_list(&params))
            .context("List mail folders error")?;
        loop {
            for folder in &page.value {
                if folder.child_folder_count.unwrap_or(0) > 0 {
                    parents.push((folder.display_name.clone(), folder.id.clone()));
                }
            }
            fold_folder_page(&mut entries, None, &page.value);
            let Some(next) = page.next_link else {
                break;
            };
            page = self.folders_from_link(&next)?;
        }

        for (parent_name, parent_id) in parents {
            let mut page = self
                .op(|client| client.mail_child_folders_list(&parent_id, &params))
                .with_context(|| format!("List child folders of {parent_name} error"))?;
            loop {
                fold_folder_page(&mut entries, Some(&parent_name), &page.value);
                let Some(next) = page.next_link else {
                    break;
                };
                page = self.folders_from_link(&next)?;
            }
        }

        self.folders = entries.iter().cloned().collect();

        debug!("end of graph folder listing");
        trace!("folders: {:?}", self.folders.keys());
        Ok(entries)
    }

    /// Follows an OData next link of a folder listing.
    fn folders_from_link(&mut self, link: &str) -> Result<MsgraphMailFoldersListResponse> {
        let url = Url::parse(link).context("Cannot parse the folder paging link")?;
        self.op(|client| {
            let coroutine =
                MsgraphSend::<MsgraphMailFoldersListResponse>::get(&client.auth, url.clone());
            client.run(coroutine)
        })
        .context("Follow folder paging link error")
    }

    /// Resolves a collection name to its Graph folder id, case-insensitively,
    /// refreshing the folder map on a miss.
    fn folder_id(&mut self, name: &str) -> Result<String> {
        if let Some(id) = lookup_folder(&self.folders, name) {
            return Ok(id);
        }
        self.list_folders()?;
        lookup_folder(&self.folders, name).with_context(|| format!("Unknown Graph folder {name}"))
    }

    /// Fetches envelopes for a message-id set, one summary get a message,
    /// its size expanded. The `Meta` tier is an upgrade a frontend asks for
    /// now and then, every listed member arriving named.
    fn fetch_envelopes(&mut self, _mailbox: &str, ids: &[&str]) -> Result<Vec<ItemSummary>> {
        let user = user_path(&self.inner.user_id);
        let mut envelopes = Vec::with_capacity(ids.len());
        for id in ids {
            let url = Url::parse(MSGRAPH_API_BASE)
                .and_then(|base| base.join(&format!("{user}/messages/{id}")))
                .map(|mut url| {
                    url.query_pairs_mut()
                        .append_pair("$select", mail::SUMMARY_SELECT)
                        .append_pair("$expand", SIZE_EXPAND);
                    url
                })
                .with_context(|| format!("Cannot build the URL of message {id}"))?;
            let message = self
                .op(|client| {
                    let coroutine = MsgraphSend::<MsgraphMessage>::get(&client.auth, url.clone());
                    client.run(coroutine)
                })
                .with_context(|| format!("Get message {id} error"))?;
            envelopes.push(message_envelope(id, &message));
        }
        Ok(envelopes)
    }

    /// Streams the bodies of a message-id set, each committed as soon as it
    /// is decoded, in requests planned by the size Graph states for each
    /// ([`plan`]): JSON batches of up to [`MSGRAPH_BATCH_MAX_REQUESTS`] raw
    /// MIME gets and [`BATCH_BYTES`] stated, a message past [`ALONE_BYTES`]
    /// or of no stated size read alone. A size neither the listings of the
    /// run nor the store (`stored`) state is asked for first, twenty ids a
    /// batch, when more than one message lacks one.
    ///
    /// Answers the messages Graph answered 404, gone since they were
    /// listed. A body still missing after [`batch_get`](Self::batch_get)'s
    /// retries, refused for another reason, or failing alone, is left out:
    /// the caller fetches every handle the fetch did not answer one by
    /// one, which surfaces its error.
    fn fetch_messages<S: Write>(
        &mut self,
        _mailbox: &str,
        ids: &[&str],
        stored: StoredSize<'_>,
        mut open: impl FnMut(&str) -> std::io::Result<S>,
        mut done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<Vec<String>> {
        let user = user_path(&self.inner.user_id);

        // NOTE: the session's own size first, which also takes it out of
        // the shared hints, then the store's, a resumed hydration's.
        let mut sizes: HashMap<&str, u64> = ids
            .iter()
            .filter_map(|id| {
                let size = self
                    .sizes
                    .take(id)
                    .or_else(|| stored(id).filter(|size| *size > 0))?;
                Some((*id, size))
            })
            .collect();
        let unstated: Vec<&str> = ids
            .iter()
            .copied()
            .filter(|id| !sizes.contains_key(id))
            .collect();
        let mut gone: Vec<String> = Vec::new();
        if unstated.len() > 1 {
            sizes.extend(self.stated_sizes(&unstated, &mut gone));
        }

        let mut commit = |id: &str, raw: &[u8]| -> Result<()> {
            let mut sink = open(id).with_context(|| format!("Open body sink for {id} error"))?;
            sink.write_all(raw)
                .with_context(|| format!("Store body {id} error"))?;
            done(id, None, sink).with_context(|| format!("Commit body {id} error"))
        };

        let asked: Vec<&str> = ids
            .iter()
            .copied()
            .filter(|id| !gone.iter().any(|gone| gone == id))
            .collect();
        for fetch in plan(&asked, |id| sizes.get(id).copied()) {
            match fetch {
                Fetch::Alone(id) => match self.message_raw(id) {
                    Ok(raw) => commit(id, &raw)?,
                    Err(err) if chain_not_found(&err) => {
                        debug!("message {id} gone before its body was read");
                        gone.push(id.to_owned());
                    }
                    Err(err) => debug!("body of {id} not read alone, left out: {err:#}"),
                },
                Fetch::Batch(batch) => {
                    let stated: u64 = batch.iter().filter_map(|id| sizes.get(id)).sum();
                    let mut decoded = 0;
                    let mut largest = 0;
                    let answer = self
                        .batch_get(
                            &batch,
                            |id| format!("/{user}/messages/{id}/$value"),
                            raw_body,
                            |id, raw: Vec<u8>| {
                                decoded += raw.len();
                                largest = largest.max(raw.len());
                                commit(id, &raw)
                            },
                        )
                        .context("Send raw message batch error")?;
                    debug!(
                        "graph body batch of {}: {stated} octets stated, {} base64 octets held at most, {decoded} decoded, largest {largest}",
                        batch.len(),
                        answer.held,
                    );
                    gone.extend(answer.gone.iter().map(|id| id.to_string()));
                }
            }
        }

        Ok(gone)
    }

    /// The size Graph states for each of `ids`, read twenty a batch with
    /// the id alone; a message stating none, or a batch that failed, is
    /// left out (read alone), and one Graph answers 404 is added to `gone`.
    fn stated_sizes<'a>(
        &mut self,
        ids: &[&'a str],
        gone: &mut Vec<String>,
    ) -> HashMap<&'a str, u64> {
        let user = user_path(&self.inner.user_id);
        let mut sizes = HashMap::with_capacity(ids.len());

        for chunk in ids.chunks(MSGRAPH_BATCH_MAX_REQUESTS) {
            let asked = self.batch_get(
                chunk,
                |id| {
                    batch_url(
                        &format!("{user}/messages/{id}"),
                        &[("$select", "id"), ("$expand", SIZE_EXPAND)],
                    )
                },
                |_, body| {
                    let message = serde_json::from_value::<MsgraphMessage>(body?).ok()?;
                    message_size(&message)
                },
                |id, size| {
                    sizes.insert(id, size);
                    Ok(())
                },
            );
            match asked {
                Ok(answer) => gone.extend(answer.gone.iter().map(|id| id.to_string())),
                Err(err) => debug!(
                    "sizes of {} messages not read, each read alone: {err:#}",
                    chunk.len()
                ),
            }
        }

        sizes
    }

    /// Sends the gets of `ids` (at most [`MSGRAPH_BATCH_MAX_REQUESTS`]) in
    /// one JSON batch, each at the URL `url` builds for its id, its body
    /// read by `decode` and handed to `deliver` before the next is read, so
    /// a batch holds no more than its answer and one decoded body.
    ///
    /// A request Graph throttles (429, or a common 5xx) is sent again in a
    /// later batch once its own `Retry-After` is over (else after a back-off
    /// doubling from a second), at most [`BATCH_RETRY_ROUNDS`] times; still
    /// throttled then, or asking to wait more than the source's longest wait
    /// (`sync --max-wait`),
    /// it is answered under `retry`, and a throttle (429, 503) gives the
    /// source up until its wait is over, every connection stopping with it.
    /// A 404 is answered under `gone`; any other refusal is left out.
    fn batch_get<'a, T>(
        &mut self,
        ids: &[&'a str],
        url: impl Fn(&str) -> String,
        decode: impl Fn(&str, Option<serde_json::Value>) -> Option<T>,
        mut deliver: impl FnMut(&'a str, T) -> Result<()>,
    ) -> Result<BatchAnswer<'a>> {
        let mut out = BatchAnswer::default();
        let start = Instant::now();
        let mut pending: Vec<Pending<'a>> = ids
            .iter()
            .enumerate()
            .map(|(index, id)| Pending {
                index,
                id,
                due: start,
                sent: 0,
            })
            .collect();

        while !pending.is_empty() {
            // NOTE: a source that gave up sends nothing until its wait is
            // over, what is left going to the next run.
            if self.throttle.blocked().is_some() {
                break;
            }

            let first = pending.iter().map(|p| p.due).min().unwrap_or(start);
            let wait = first.saturating_duration_since(Instant::now());
            if !wait.is_zero() {
                warn!(
                    "graph throttled {} of {} batched requests, retrying in {}s",
                    pending.len(),
                    ids.len(),
                    wait.as_secs()
                );
                std::thread::sleep(wait);
            }

            let now = Instant::now();
            let (due, later): (Vec<_>, Vec<_>) = pending.into_iter().partition(|p| p.due <= now);
            pending = later;
            let asked: Vec<&'a str> = due.iter().map(|p| p.id).collect();

            let requests = get_requests(&asked, &url);
            let answered = match self
                .send(Request::Idempotent, true, |client| client.batch(&requests))
            {
                Ok(responses) => read_responses(&asked, responses, &decode, &mut deliver)?,
                Err(err) if is_throttled(&err) => {
                    let throttled = matches!(
                        &err,
                        MsgraphClientStdError::Send(send) if matches!(send.status(), Some(429 | 503))
                    );
                    Answered::retry_all(&asked, throttled)
                }
                Err(err) => return Err(err.into()),
            };

            out.held = out.held.max(answered.held);
            out.gone.extend(answered.gone);
            for (id, stated, throttled) in answered.retry {
                let Some(mut item) = due.iter().find(|p| p.id == id).cloned() else {
                    continue;
                };
                let wait = stated.unwrap_or_else(|| Duration::from_secs(1 << item.sent.min(6)));
                item.sent += 1;
                if item.sent > BATCH_RETRY_ROUNDS || wait > self.throttle.max_wait() {
                    // NOTE: a request Graph failed to serve (500, 502, 504)
                    // is retried the same, but is no throttle: the source
                    // goes on.
                    if throttled {
                        self.throttle.give_up(SystemTime::now() + wait);
                    }
                    out.retry.push((item.index, id));
                    continue;
                }
                item.due = Instant::now() + wait;
                pending.push(item);
            }
        }

        out.retry.extend(pending.iter().map(|p| (p.index, p.id)));
        out.retry.sort_unstable();
        Ok(out)
    }

    /// Streams one message's raw RFC 5322 bytes into `sink`.
    fn get_message_stream(&mut self, _mailbox: &str, id: &str, mut sink: impl Write) -> Result<()> {
        let raw = self.message_raw(id)?;
        sink.write_all(&raw)
            .with_context(|| format!("Stream body {id} error"))?;
        Ok(())
    }

    /// Fetches the raw RFC 5322 MIME content of one message.
    fn message_raw(&mut self, id: &str) -> Result<Vec<u8>> {
        self.op(|client| client.message_get_raw(id))
            .with_context(|| format!("Get raw message {id} error"))
    }

    /// Replaces flags: `\Seen` is `isRead`, `\Flagged` the follow-up status.
    ///
    /// Only [`FlagOp::Set`] is supported, the engine pushing full flag sets.
    /// `\Draft` is read-only on Graph and other keywords have no equivalent,
    /// both ignored.
    pub fn store_flags(&mut self, ids: &[&str], flags: &[Flag], op: FlagOp) -> Result<()> {
        if self.kind != GraphKind::Mail {
            bail!(
                "Graph {} have no flags (store not supported)",
                self.kind.media_type()
            );
        }
        if !matches!(op, FlagOp::Set) {
            bail!("Graph flag updates only support a full set");
        }

        let patch = flags_patch(flags);
        for id in ids {
            match self.op(|client| client.message_update(id, &patch)) {
                Err(err) if is_not_found(&err) => {
                    debug!("message {id} gone from the server, its flags count as set");
                }
                updated => {
                    updated.with_context(|| format!("Update flags of {id} error"))?;
                }
            }
        }
        Ok(())
    }

    /// Uploads a message into the Drafts folder, then sets its flags.
    ///
    /// Graph creates every MIME message as a draft, so an append anywhere
    /// else is refused rather than filed there as a draft. The upload
    /// ignores flags, so `\Seen` and `\Flagged` are patched in after; a
    /// failed patch is only warned about, the message being created, and the
    /// next enumeration reports the flags it holds.
    fn add_message(
        &mut self,
        mailbox: &str,
        flags: &[Flag],
        mut source: impl Read,
    ) -> Result<WrittenItem> {
        let folder = self.folder_id(mailbox)?;
        if folder != self.drafts_id()? {
            bail!(
                "Graph creates every MIME message as a draft, so it appends to Drafts only, not to {mailbox}"
            );
        }

        let mut raw = Vec::new();
        source
            .read_to_end(&mut raw)
            .context("Read message to append error")?;
        let id = self
            .create(|client| client.message_create_mime(Some(&folder), &raw))
            .with_context(|| format!("Create message in {mailbox} error"))?
            .id;
        if id.is_empty() {
            bail!("Graph created a message in {mailbox} but named no id");
        }

        let patch = flags_patch(flags);
        if let Err(err) = self.op(|client| client.message_update(&id, &patch)) {
            warn!("cannot set the flags of the message created in {mailbox}: {err}");
        }

        Ok(WrittenItem { id, revision: None })
    }

    /// The id of the well-known Drafts folder, whatever its display name.
    fn drafts_id(&mut self) -> Result<String> {
        if let Some(id) = &self.drafts {
            return Ok(id.clone());
        }

        let id = self
            .op(|client| client.mail_folder_get("drafts"))
            .context("Get the Drafts folder error")?
            .id;
        self.drafts = Some(id.clone());
        Ok(id)
    }

    /// Moves messages into the folder `to`.
    ///
    /// Graph answers each move with the message's new id, which is only
    /// logged: the target's next enumeration lists the message under it, and
    /// the fetch naming it lands the move's pending create there (pimdir
    /// SYNC §5, §6), as after an IMAP `MOVE`.
    ///
    /// A message gone from the server counts as moved: a 404 is told from a
    /// destination Graph no longer knows by reading the message, the
    /// destination being resolved from the folder listing first.
    pub fn move_messages(&mut self, to: &str, ids: &[&str]) -> Result<()> {
        if self.kind != GraphKind::Mail {
            bail!(
                "Graph {} cannot move between folders here (move not supported)",
                self.kind.media_type()
            );
        }

        let folder = self.folder_id(to)?;
        for id in ids {
            match self.op(|client| client.message_move(id, &folder)) {
                Ok(moved) => debug!("moved message {id} to {to}, now {}", moved.id),
                Err(err) if is_not_found(&err) && self.message_gone(id)? => {
                    debug!("message {id} gone from the server, its move counts as done");
                }
                Err(err) => {
                    return Err(err).with_context(|| format!("Move message {id} to {to} error"));
                }
            }
        }
        Ok(())
    }

    /// Whether Graph answers that message `id` is gone (404), read by its
    /// id alone.
    fn message_gone(&mut self, id: &str) -> Result<bool> {
        let user = user_path(&self.inner.user_id);
        let mut url = Url::parse(MSGRAPH_API_BASE)
            .and_then(|base| base.join(&format!("{user}/messages/{id}")))
            .with_context(|| format!("Cannot build the URL of message {id}"))?;
        url.query_pairs_mut().append_pair("$select", "id");

        match self.op(|client| {
            let coroutine = MsgraphSend::<MsgraphMessage>::get(&client.auth, url.clone());
            client.run(coroutine)
        }) {
            Ok(_) => Ok(false),
            Err(err) if is_not_found(&err) => Ok(true),
            Err(err) => Err(err).with_context(|| format!("Get message {id} error")),
        }
    }

    /// Copies a message into the folder `to`, returning the copy's id.
    pub fn copy_message(&mut self, to: &str, id: &str) -> Result<WrittenItem> {
        if self.kind != GraphKind::Mail {
            bail!(
                "Graph {} cannot copy between folders here (copy not supported)",
                self.kind.media_type()
            );
        }

        let folder = self.folder_id(to)?;
        let copy = self
            .create(|client| client.message_copy(id, &folder))
            .with_context(|| format!("Copy message {id} to {to} error"))?;
        if copy.id.is_empty() {
            bail!("Graph copied message {id} to {to} but named no id");
        }
        debug!("copied message {id} to {to} as {}", copy.id);

        Ok(WrittenItem {
            id: copy.id,
            revision: None,
        })
    }

    /// Deletes one message by id; one gone from the server already counts
    /// as deleted.
    fn delete_message(&mut self, id: &str) -> Result<()> {
        match self.op(|client| client.message_delete(id)) {
            Err(err) if is_not_found(&err) => {
                debug!("message {id} gone from the server already, its delete counts as done");
                Ok(())
            }
            deleted => deleted
                .map(|_| ())
                .with_context(|| format!("Delete message {id} error")),
        }
    }

    /// Sends raw MIME through sendMail, which files the message in Sent itself.
    ///
    /// A `POST` that creates: never sent again on a dead connection
    /// ([`exchange`](Self::exchange)), sent again only on a 429, which Graph
    /// answers before doing anything ([`create`](Self::create)). The client
    /// error comes back unwrapped, so the caller can read the HTTP status off
    /// it, with whether the request had gone whole and no answer was read
    /// whole ([`Exchange::written`]): an answer cut short counts as none.
    /// sendMail derives the recipients from the MIME headers (Bcc included),
    /// so envelope recipients beyond the headers are lost.
    pub fn send_mime(&mut self, raw: &[u8]) -> Result<(), SendMimeFailure> {
        let error = match self.create(|client| client.mail_send_mime(raw)) {
            Ok(_) => return Ok(()),
            Err(error) => error,
        };
        // NOTE: the record is the last attempt's, cleared before a reopen
        // that failed. A status or a redirect is a verdict; an answer that
        // did not parse may be a 2xx, so it is none.
        let verdict = matches!(
            &error,
            MsgraphClientStdError::Send(
                MsgraphSendError::Api { .. } | MsgraphSendError::UnexpectedRedirect
            )
        );
        let unanswered = !verdict && wire::read(&self.exchange, Exchange::written);
        Err(SendMimeFailure { error, unanswered })
    }
}

/// A `sendMail` that failed.
#[derive(Debug)]
pub struct SendMimeFailure {
    /// The client error, its status read off it.
    pub error: MsgraphClientStdError,
    /// Whether the request had been written whole and no answer read whole:
    /// the message may have been taken.
    pub unanswered: bool,
}

/// How many times a throttled body request is sent again in a later batch.
const BATCH_RETRY_ROUNDS: u32 = 4;

/// The gets of a batch, each at the URL `url` builds for its id, each
/// request id being the index of its id in `ids`.
fn get_requests(ids: &[&str], url: impl Fn(&str) -> String) -> Vec<MsgraphBatchRequest> {
    ids.iter()
        .enumerate()
        .map(|(index, id)| MsgraphBatchRequest {
            id: index.to_string(),
            method: String::from("GET"),
            url: url(id),
            ..Default::default()
        })
        .collect()
}

/// The raw MIME gets of a batch, each request id being the index of its
/// message id in `ids`.
/// A mail session over the connections `connect` opens, for a test of
/// another module driving several connections of one source.
#[cfg(test)]
impl GraphClient {
    pub(crate) fn over_streams(
        connect: impl FnMut() -> Result<Box<dyn MsgraphStream>> + Send + 'static,
        sizes: SizeHints,
        throttle: Arc<Throttle>,
    ) -> Result<Self> {
        let token = SecretString::from("token");
        Self::over(
            GraphKind::Mail,
            &token,
            "me",
            Box::new(connect),
            sizes,
            throttle,
        )
    }
}

#[cfg(test)]
impl SizeHints {
    /// Notes `sizes` as a listing would.
    pub(crate) fn noted(&self, sizes: impl IntoIterator<Item = (String, u64)>) {
        self.note(sizes);
    }
}

#[cfg(test)]
fn raw_requests(user: &str, ids: &[&str]) -> Vec<MsgraphBatchRequest> {
    get_requests(ids, |id| format!("/{user}/messages/{id}/$value"))
}

/// The most octets of stated message size one batch of bodies asks for:
/// its answer, base64 and JSON, about a third more, is held whole while it
/// is read, then each body decoded in turn.
const BATCH_BYTES: u64 = 4 * 1024 * 1024;

/// A message stated larger than this is read alone, by a plain get.
const ALONE_BYTES: u64 = 1024 * 1024;

/// The most sizes a source keeps between its listing and its bodies:
/// beyond, a message is planned as one of no stated size.
const MAX_SIZE_HINTS: usize = 100_000;

/// The `$expand` asking Graph for the MAPI message size
/// (`PidTagMessageSize`, `Integer 0x0E08`), the size Exchange states for a
/// message as IMAP's `RFC822.SIZE` does (io-msgraph `fa73f04`).
pub(super) const SIZE_EXPAND: &str =
    "singleValueExtendedProperties($filter=id eq 'Integer 0x0E08')";

/// The sizes a source's listings stated, by message id, until its bodies
/// are planned: shared by every connection opened from the source, since
/// the connection listing a folder is rarely the one downloading it.
#[derive(Clone, Debug, Default)]
pub struct SizeHints(Arc<Mutex<HashMap<String, u64>>>);

impl SizeHints {
    /// Notes `sizes`, up to [`MAX_SIZE_HINTS`] in all.
    fn note(&self, sizes: impl IntoIterator<Item = (String, u64)>) {
        let mut hints = self.0.lock().unwrap_or_else(|err| err.into_inner());
        for (id, size) in sizes {
            if hints.len() >= MAX_SIZE_HINTS && !hints.contains_key(&id) {
                break;
            }
            hints.insert(id, size);
        }
    }

    /// Takes the size noted for `id`.
    fn take(&self, id: &str) -> Option<u64> {
        let mut hints = self.0.lock().unwrap_or_else(|err| err.into_inner());
        hints.remove(id)
    }
}

/// The MAPI size a message states, when [`SIZE_EXPAND`] asked for it.
fn message_size(message: &MsgraphMessage) -> Option<u64> {
    message
        .single_value_extended_properties
        .iter()
        .find(|property| property.id.eq_ignore_ascii_case("Integer 0xe08"))
        .or_else(|| message.single_value_extended_properties.first())
        .and_then(|property| property.value.trim().parse().ok())
}

/// One request of a body fetch.
#[derive(Debug, Eq, PartialEq)]
enum Fetch<'a> {
    /// A JSON batch of raw gets.
    Batch(Vec<&'a str>),
    /// One raw get.
    Alone(&'a str),
}

/// Plans the bodies of `ids` by the size `size` states for each, in their
/// order: batches of at most [`MSGRAPH_BATCH_MAX_REQUESTS`] gets and
/// [`BATCH_BYTES`] stated, a message past [`ALONE_BYTES`] or of no stated
/// size read alone where it comes.
fn plan<'a>(ids: &[&'a str], size: impl Fn(&str) -> Option<u64>) -> Vec<Fetch<'a>> {
    let mut plan = Vec::new();
    let mut batch: Vec<&str> = Vec::new();
    let mut bytes = 0;

    for id in ids {
        let Some(size) = size(id).filter(|size| *size <= ALONE_BYTES) else {
            plan.push(Fetch::Alone(id));
            continue;
        };
        if batch.len() == MSGRAPH_BATCH_MAX_REQUESTS || bytes + size > BATCH_BYTES {
            plan.push(Fetch::Batch(std::mem::take(&mut batch)));
            bytes = 0;
        }
        batch.push(id);
        bytes += size;
    }
    if !batch.is_empty() {
        plan.push(Fetch::Batch(batch));
    }

    plan
}

/// The address of a get inside a JSON batch: `path` under the API version,
/// with `query` percent-encoded.
fn batch_url(path: &str, query: &[(&str, &str)]) -> String {
    let base = Url::parse(MSGRAPH_API_BASE).expect("the Graph API base URL parses");
    let Ok(mut url) = base.join(path) else {
        return format!("/{path}");
    };
    url.query_pairs_mut().extend_pairs(query);
    let path = url.path().strip_prefix(base.path()).unwrap_or(url.path());
    match url.query() {
        Some(query) => format!("/{path}?{query}"),
        None => format!("/{path}"),
    }
}

/// A get of a batch still to answer.
#[derive(Clone, Debug)]
struct Pending<'a> {
    /// Its place among the ids asked.
    index: usize,
    id: &'a str,
    /// When it may be sent: its `Retry-After` over.
    due: Instant,
    /// How many times Graph throttled it.
    sent: u32,
}

/// What [`GraphClient::batch_get`] answered beyond what it delivered.
#[derive(Debug, Default)]
struct BatchAnswer<'a> {
    /// The ids still throttled, by their place among the ids asked.
    retry: Vec<(usize, &'a str)>,
    /// The ids Graph answered 404, gone since they were listed.
    gone: Vec<&'a str>,
    /// The most base64 and JSON octets one answer held while it was read.
    held: usize,
}

/// What one batch answered beyond what it delivered.
#[derive(Debug, Default, PartialEq)]
struct Answered<'a> {
    /// The ids Graph throttled, to send again, in request order, each with
    /// the `Retry-After` it stated and whether it was a throttle (429,
    /// 503) rather than a failure (500, 502, 504): only a throttle gives
    /// the source up.
    retry: Vec<(&'a str, Option<Duration>, bool)>,
    /// The ids Graph answered 404, gone since they were listed.
    gone: Vec<&'a str>,
    /// The body octets the answer held (base64 text, JSON).
    held: usize,
}

impl<'a> Answered<'a> {
    /// A batch throttled as a whole: every request to send again.
    fn retry_all(ids: &[&'a str], throttled: bool) -> Self {
        Self {
            retry: ids.iter().map(|id| (*id, None, throttled)).collect(),
            ..Default::default()
        }
    }
}

/// Reads the responses of a batch of raw gets sent for `ids`, collecting
/// the bodies.
#[cfg(test)]
fn read_raw_responses<'a>(
    ids: &[&'a str],
    responses: MsgraphBatchResponses,
) -> (HashMap<&'a str, Vec<u8>>, Answered<'a>) {
    let mut bodies = HashMap::new();
    let answered = read_responses(ids, responses, raw_body, &mut |id, body| {
        bodies.insert(id, body);
        Ok(())
    })
    .unwrap();
    (bodies, answered)
}

/// Reads the responses of a batch of gets sent for `ids`, matched by
/// request id (Graph answers in any order), one at a time: a 2xx body is
/// read by `decode` and handed to `deliver`, its JSON dropped before the
/// next is read; a throttled request is kept to retry with its own
/// `Retry-After`, a 404 named gone; any other answer, or a body `decode`
/// refuses, is left out.
fn read_responses<'a, T>(
    ids: &[&'a str],
    responses: MsgraphBatchResponses,
    decode: impl Fn(&str, Option<serde_json::Value>) -> Option<T>,
    deliver: &mut impl FnMut(&'a str, T) -> Result<()>,
) -> Result<Answered<'a>> {
    let mut answer = Answered {
        held: responses
            .responses
            .iter()
            .filter_map(|response| response.body.as_ref())
            .map(|body| match body {
                serde_json::Value::String(text) => text.len(),
                other => other.to_string().len(),
            })
            .sum(),
        ..Default::default()
    };
    let mut retry = Vec::new();

    for response in responses.responses {
        let Some(index) = response.id.parse::<usize>().ok().filter(|i| *i < ids.len()) else {
            warn!(
                "graph batch answered an unknown request id {:?}",
                response.id
            );
            continue;
        };
        let id = ids[index];

        if matches!(response.status, 429 | 500 | 502 | 503 | 504) {
            let after = response
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("retry-after"))
                .and_then(|(_, value)| throttle::retry_after(value));
            retry.push((index, after, matches!(response.status, 429 | 503)));
            continue;
        }

        if response.status == 404 {
            answer.gone.push(id);
            continue;
        }

        if !(200..300).contains(&response.status) {
            debug!("graph batch get of {id} answered {}", response.status);
            continue;
        }

        if let Some(body) = decode(id, response.body) {
            deliver(id, body)?;
        }
    }

    retry.sort_unstable_by_key(|(index, _, _)| *index);
    retry.dedup_by_key(|(index, _, _)| *index);
    answer.retry = retry
        .into_iter()
        .map(|(index, after, throttled)| (ids[index], after, throttled))
        .collect();
    Ok(answer)
}

/// The raw MIME of a batch body: base64 text, as Graph answers a body
/// that is not JSON.
fn raw_body(id: &str, body: Option<serde_json::Value>) -> Option<Vec<u8>> {
    let decoded = match &body {
        Some(serde_json::Value::String(encoded)) => decode_batch_body(encoded),
        _ => None,
    };
    if decoded.is_none() {
        warn!("graph batch body of {id} is not base64 text");
    }
    decoded
}

/// Decodes a non-JSON batch body: base64 in the standard alphabet, as Graph
/// answers it, or the URL-safe one its batching docs name for bodies.
fn decode_batch_body(encoded: &str) -> Option<Vec<u8>> {
    STANDARD
        .decode(encoded)
        .or_else(|_| URL_SAFE.decode(encoded))
        .or_else(|_| URL_SAFE_NO_PAD.decode(encoded))
        .ok()
}

/// Whether a client error is Graph throttling the whole batch call.
fn is_throttled(err: &MsgraphClientStdError) -> bool {
    matches!(err, MsgraphClientStdError::Send(send) if send.is_retryable())
}

/// Whether a client error is Graph answering that what the request names
/// does not exist (404).
fn is_not_found(err: &MsgraphClientStdError) -> bool {
    matches!(err, MsgraphClientStdError::Send(send) if send.status() == Some(404))
}

/// Whether an error chain carries a Graph 404 (see [`is_not_found`]).
fn chain_not_found(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<MsgraphClientStdError>()
            .is_some_and(is_not_found)
    })
}

/// Whether a client error is an expired delta link (HTTP 410), the signal to
/// restart a full round.
fn is_expired_link(err: &MsgraphClientStdError) -> bool {
    matches!(err, MsgraphClientStdError::Send(send) if send.status() == Some(410))
}

/// Names the listed folders whose id a well-known lookup answered with, by
/// the role it states. A well-known folder outside the listing (deeper than
/// two levels) names nothing.
fn roles_by_id(
    folders: &[(String, String)],
    stated: &[(String, &str)],
) -> BTreeMap<String, String> {
    folders
        .iter()
        .filter_map(|(name, id)| {
            let (_, role) = stated.iter().find(|(stated, _)| stated == id)?;
            Some((name.clone(), (*role).to_owned()))
        })
        .collect()
}

/// Folds one folder listing page into `(name, id)` entries, prefixing child
/// folders with their parent name. Folders missing a name or an id are skipped.
fn fold_folder_page(
    entries: &mut Vec<(String, String)>,
    parent: Option<&str>,
    folders: &[MsgraphMailFolder],
) {
    for folder in folders {
        if folder.id.is_empty() || folder.display_name.is_empty() {
            continue;
        }
        let name = match parent {
            Some(parent) => format!("{parent}/{}", folder.display_name),
            None => folder.display_name.clone(),
        };
        entries.push((name, folder.id.clone()));
    }
}

/// Finds a folder id by mailbox name, case-insensitively, as the sync matches
/// mailbox names too.
fn lookup_folder(folders: &HashMap<String, String>, name: &str) -> Option<String> {
    folders
        .iter()
        .find(|(folder, _)| folder.eq_ignore_ascii_case(name))
        .map(|(_, id)| id.clone())
}

/// Maps a delta row to the shared flag set. `\Answered` and `\Deleted` have no
/// Graph delta equivalent and are never produced.
fn message_flags(message: &MsgraphMessage) -> std::collections::BTreeSet<Flag> {
    mail::Marks::of(message).flags()
}

/// The `message_update` patch replacing a message's flags. Every other field
/// stays absent, so the PATCH touches nothing else.
fn flags_patch(flags: &[Flag]) -> MsgraphMessage {
    let seen = flags.iter().any(|f| f.iana() == Some(IanaFlag::Seen));
    let flagged = flags.iter().any(|f| f.iana() == Some(IanaFlag::Flagged));
    MsgraphMessage {
        is_read: Some(seen),
        flag: Some(MsgraphFollowupFlag {
            flag_status: Some(if flagged {
                MsgraphFlagStatus::Flagged
            } else {
                MsgraphFlagStatus::NotFlagged
            }),
        }),
        ..Default::default()
    }
}

/// The author-claimed date of a delta row: `sentDateTime`, Graph's reading
/// of the `Date` header, which pimdir's mail `date` column holds (STORAGE
/// Annex A.1). Never `receivedDateTime`, the server's arrival time: absent
/// or unparseable, the date is `None` (a `NULL` column), as a missing `Date`.
fn message_date(message: &MsgraphMessage) -> Option<DateTime<FixedOffset>> {
    let raw = message.sent_date_time.as_deref()?;
    DateTime::parse_from_rfc3339(raw).ok()
}

/// Folds a delta row into a shared [`ItemSummary`] for the `Meta` tier.
fn message_envelope(id: &str, message: &MsgraphMessage) -> ItemSummary {
    let address = |recipient: &io_msgraph::v1::rest::users::messages::MsgraphRecipient| {
        crate::item::address::Address {
            name: recipient.email_address.name.clone(),
            email: recipient.email_address.address.clone().unwrap_or_default(),
        }
    };
    ItemSummary {
        id: id.to_owned(),
        message_id: message
            .internet_message_id
            .as_deref()
            .and_then(normalize_message_id),
        in_reply_to: Vec::new(),
        flags: message_flags(message),
        subject: message.subject.clone().unwrap_or_default(),
        from: message.from.as_ref().map(address).into_iter().collect(),
        to: message.to_recipients.iter().map(address).collect(),
        cc: message.cc_recipients.iter().map(address).collect(),
        bcc: Vec::new(),
        date: message_date(message),
        // NOTE: the MAPI size, when the read expanded it; else 0, filled
        // from the blob length at the `Full` tier.
        size: message_size(message).unwrap_or(0),
        // NOTE: Graph states the mark itself (pimdir STORAGE Annex A.1),
        // which the walk of the parts replaces once the body is in.
        has_attachment: message.has_attachments,
    }
}

/// Encodes the delta link into checkpoint bytes (plain UTF-8).
fn encode_checkpoint(link: &str) -> Vec<u8> {
    link.as_bytes().to_vec()
}

/// Decodes checkpoint bytes back into the delta link; `None` for an absent,
/// empty or non-UTF-8 checkpoint, which forces a fresh full round.
fn decode_checkpoint(bytes: &[u8]) -> Option<String> {
    let link = std::str::from_utf8(bytes).ok()?;
    (!link.is_empty()).then(|| link.to_owned())
}

#[cfg(test)]
mod tests {
    use io_msgraph::v1::send::MsgraphSendError;

    use super::*;

    #[test]
    fn a_listed_folder_takes_the_role_its_well_known_id_states() {
        let folders = [
            ("Boîte de réception".to_owned(), "AQ-inbox".to_owned()),
            ("Éléments envoyés".to_owned(), "AQ-sent".to_owned()),
            ("Brouillons".to_owned(), "AQ-drafts".to_owned()),
            ("Sent".to_owned(), "AQ-other".to_owned()),
        ];
        let stated = [
            ("AQ-inbox".to_owned(), "inbox"),
            ("AQ-sent".to_owned(), "sent"),
            ("AQ-drafts".to_owned(), "drafts"),
            ("AQ-deep".to_owned(), "archive"),
        ];
        let roles = roles_by_id(&folders, &stated);
        assert_eq!(
            roles.get("Boîte de réception").map(String::as_str),
            Some("inbox")
        );
        assert_eq!(
            roles.get("Éléments envoyés").map(String::as_str),
            Some("sent")
        );
        assert_eq!(roles.get("Brouillons").map(String::as_str), Some("drafts"));
        assert_eq!(roles.get("Sent"), None);
        assert_eq!(roles.len(), 3);
    }

    /// A delta row fixture as Graph would serialize it, exercising the serde
    /// shape along the way.
    fn fixture_row() -> MsgraphMessage {
        serde_json::from_str(
            r#"{
                "id": "AAMkAD-abc",
                "subject": "Hello",
                "from": {"emailAddress": {"name": "Alice", "address": "alice@example.org"}},
                "toRecipients": [{"emailAddress": {"address": "bob@example.org"}}],
                "sentDateTime": "2026-07-06T12:00:00Z",
                "receivedDateTime": "2026-07-06T12:05:00Z",
                "internetMessageId": "<m1@example.org>",
                "isRead": true,
                "isDraft": false,
                "flag": {"flagStatus": "flagged"},
                "parentFolderId": "AQMkAD-inbox"
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn flags_map_to_iana_wire_spellings() {
        let message = fixture_row();
        let flags = message_flags(&message);
        assert!(flags.contains(&Flag::from_iana(IanaFlag::Seen)));
        assert!(flags.contains(&Flag::from_iana(IanaFlag::Flagged)));
        assert!(!flags.contains(&Flag::from_iana(IanaFlag::Draft)));

        let bare = MsgraphMessage::default();
        assert!(message_flags(&bare).is_empty());

        let draft: MsgraphMessage = serde_json::from_str(
            r#"{"id": "x", "isRead": false, "isDraft": true, "flag": {"flagStatus": "notFlagged"}}"#,
        )
        .unwrap();
        let flags = message_flags(&draft);
        assert!(flags.contains(&Flag::from_iana(IanaFlag::Draft)));
        assert!(!flags.contains(&Flag::from_iana(IanaFlag::Seen)));
        assert!(!flags.contains(&Flag::from_iana(IanaFlag::Flagged)));
    }

    #[test]
    fn a_delta_row_folds_into_an_envelope() {
        let message = fixture_row();
        let env = message_envelope("AAMkAD-abc", &message);
        assert_eq!(env.id, "AAMkAD-abc");
        assert_eq!(env.message_id.as_deref(), Some("m1@example.org"));
        assert_eq!(env.subject, "Hello");
        assert_eq!(env.from[0].email, "alice@example.org");
        assert_eq!(env.to[0].email, "bob@example.org");
        assert_eq!(
            env.date
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-07-06T12:00:00Z",
            "the date is sentDateTime, never receivedDateTime"
        );
        assert_eq!(env.size, 0, "Graph exposes no RFC 5322 size");
    }

    #[test]
    fn a_row_without_sent_date_has_no_date() {
        let message: MsgraphMessage =
            serde_json::from_str(r#"{"id": "x", "receivedDateTime": "2026-07-06T12:05:00Z"}"#)
                .unwrap();
        assert_eq!(message_date(&message), None);
    }

    #[test]
    fn a_flags_patch_touches_only_the_flag_fields() {
        let patch = flags_patch(&[Flag::from_iana(IanaFlag::Seen)]);
        assert_eq!(patch.is_read, Some(true));
        assert_eq!(
            patch.flag.as_ref().and_then(|f| f.flag_status),
            Some(MsgraphFlagStatus::NotFlagged)
        );
        let body = serde_json::to_value(&patch).unwrap();
        assert_eq!(
            body.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["flag", "isRead"]
        );

        let patch = flags_patch(&[Flag::from_iana(IanaFlag::Flagged)]);
        assert_eq!(patch.is_read, Some(false));
        assert_eq!(
            patch.flag.as_ref().and_then(|f| f.flag_status),
            Some(MsgraphFlagStatus::Flagged)
        );
    }

    #[test]
    fn only_a_410_counts_as_an_expired_link() {
        let expired = MsgraphClientStdError::Send(MsgraphSendError::Api {
            status: 410,
            code: "syncStateNotFound".into(),
            message: "gone".into(),
        });
        assert!(is_expired_link(&expired));

        let denied = MsgraphClientStdError::Send(MsgraphSendError::Api {
            status: 403,
            code: "accessDenied".into(),
            message: "no".into(),
        });
        assert!(!is_expired_link(&denied));
        let io = MsgraphClientStdError::Io(std::io::Error::other("reset"));
        assert!(!is_expired_link(&io));
    }

    #[test]
    fn checkpoint_round_trips_the_delta_link() {
        let link =
            "https://graph.microsoft.com/v1.0/me/mailFolders/x/messages/delta?$deltatoken=abc";
        assert_eq!(
            decode_checkpoint(&encode_checkpoint(link)).as_deref(),
            Some(link)
        );
        assert_eq!(decode_checkpoint(&[]), None);
        assert_eq!(decode_checkpoint(&[0xff, 0xfe]), None);
    }

    #[test]
    fn folder_map_folds_pages_and_child_levels() {
        let page: MsgraphMailFoldersListResponse = serde_json::from_str(
            r#"{
                "value": [
                    {"id": "id-inbox", "displayName": "Inbox", "childFolderCount": 1},
                    {"id": "id-archive", "displayName": "Archive", "childFolderCount": 0},
                    {"id": "id-anon", "displayName": ""}
                ],
                "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/mailFolders?$skip=3"
            }"#,
        )
        .unwrap();
        assert!(page.next_link.is_some());

        let mut entries = Vec::new();
        fold_folder_page(&mut entries, None, &page.value);
        fold_folder_page(
            &mut entries,
            Some("Inbox"),
            &[MsgraphMailFolder {
                id: String::from("id-child"),
                display_name: String::from("Receipts"),
                ..Default::default()
            }],
        );

        let map: HashMap<String, String> = entries.into_iter().collect();
        assert_eq!(map.get("Inbox").map(String::as_str), Some("id-inbox"));
        assert_eq!(map.get("Archive").map(String::as_str), Some("id-archive"));
        assert_eq!(
            map.get("Inbox/Receipts").map(String::as_str),
            Some("id-child")
        );
        assert_eq!(map.len(), 3);

        assert_eq!(
            lookup_folder(&map, "inbox").as_deref(),
            Some("id-inbox"),
            "lookup is case-insensitive"
        );
        assert_eq!(lookup_folder(&map, "Missing"), None);
    }

    #[test]
    fn a_batch_of_raw_gets_names_each_message_by_its_index() {
        let requests = raw_requests("users/u@example.test", &["AAMk-a=", "AAMk-b="]);

        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].id, "0");
        assert_eq!(requests[0].method, "GET");
        assert_eq!(
            requests[0].url,
            "/users/u@example.test/messages/AAMk-a=/$value"
        );
        assert_eq!(requests[1].id, "1");
        assert_eq!(
            requests[1].url,
            "/users/u@example.test/messages/AAMk-b=/$value"
        );
    }

    #[test]
    fn batch_bodies_decode_out_of_order_and_throttled_ones_are_retried() {
        let ids = ["m0", "m1", "m2", "m3", "m4"];
        let raw0 = b"Subject: zero\r\n\r\nbody".to_vec();
        let raw2 = b"Subject: two\r\n\r\n\xff\xfe".to_vec();
        let responses: MsgraphBatchResponses = serde_json::from_value(serde_json::json!({
            "responses": [
                {
                    "id": "2",
                    "status": 200,
                    "headers": { "Content-Type": "text/plain" },
                    "body": URL_SAFE.encode(&raw2),
                },
                {
                    "id": "4",
                    "status": 429,
                    "headers": { "Retry-After": "7" },
                    "body": { "error": { "code": "TooManyRequests" } },
                },
                {
                    "id": "0",
                    "status": 200,
                    "headers": { "Content-Type": "text/plain" },
                    "body": STANDARD.encode(&raw0),
                },
                {
                    "id": "3",
                    "status": 503,
                    "headers": { "retry-after": "3" },
                },
                {
                    "id": "1",
                    "status": 404,
                    "body": { "error": { "code": "ErrorItemNotFound" } },
                },
                { "id": "9", "status": 200, "body": "AAAA" },
            ]
        }))
        .unwrap();

        let (bodies, answer) = read_raw_responses(&ids, responses);

        assert_eq!(bodies.len(), 2, "a refused get is left out");
        assert_eq!(bodies["m0"], raw0);
        assert_eq!(bodies["m2"], raw2);
        assert_eq!(
            answer.retry,
            [
                ("m3", Some(Duration::from_secs(3)), true),
                ("m4", Some(Duration::from_secs(7)), true)
            ],
            "in request order, each with its own Retry-After"
        );
        assert_eq!(answer.gone, ["m1"], "a 404 is gone since it was listed");
    }

    #[test]
    fn a_batch_body_that_is_not_base64_text_is_left_out() {
        let ids = ["m0", "m1"];
        let responses: MsgraphBatchResponses = serde_json::from_value(serde_json::json!({
            "responses": [
                { "id": "0", "status": 200, "body": { "subject": "json" } },
                { "id": "1", "status": 200, "body": "not base64!" },
            ]
        }))
        .unwrap();

        let (bodies, answer) = read_raw_responses(&ids, responses);

        assert!(bodies.is_empty());
        assert!(answer.retry.is_empty());
    }

    #[test]
    fn a_batch_throttled_as_a_whole_is_sent_again() {
        let throttled = MsgraphClientStdError::Send(MsgraphSendError::Api {
            status: 429,
            code: String::from("TooManyRequests"),
            message: String::new(),
        });
        let refused = MsgraphClientStdError::Send(MsgraphSendError::Api {
            status: 403,
            code: String::from("ErrorAccessDenied"),
            message: String::new(),
        });

        assert!(is_throttled(&throttled));
        assert!(!is_throttled(&refused));
        assert_eq!(
            Answered::retry_all(&["m0", "m1"], true).retry,
            [("m0", None, true), ("m1", None, true)]
        );
    }

    /// Graph's own `hasAttachments` is the mark, both ways, until the body
    /// is read; a row that states nothing leaves it unknown.
    #[test]
    fn the_attachment_mark_is_the_one_graph_states() {
        let mut with = fixture_row();
        with.has_attachments = Some(true);
        assert_eq!(message_envelope("a", &with).has_attachment, Some(true));

        let mut without = fixture_row();
        without.has_attachments = Some(false);
        assert_eq!(message_envelope("b", &without).has_attachment, Some(false));

        assert_eq!(message_envelope("c", &fixture_row()).has_attachment, None);
    }

    /// A body Graph failed to serve (500, 502, 504) is sent again, but only
    /// a 429 or a 503 is a throttle, the one that gives the source up.
    #[test]
    fn a_failed_batch_body_is_retried_without_reading_as_a_throttle() {
        let responses: MsgraphBatchResponses = serde_json::from_value(serde_json::json!({
            "responses": [
                { "id": "0", "status": 500 },
                { "id": "1", "status": 504 },
            ]
        }))
        .unwrap();
        let (_, answer) = read_raw_responses(&["m0", "m1"], responses);
        assert_eq!(answer.retry, [("m0", None, false), ("m1", None, false)]);

        let responses: MsgraphBatchResponses = serde_json::from_value(serde_json::json!({
            "responses": [
                { "id": "0", "status": 502 },
                { "id": "1", "status": 503 },
            ]
        }))
        .unwrap();
        let (_, answer) = read_raw_responses(&["m0", "m1"], responses);
        assert_eq!(answer.retry, [("m0", None, false), ("m1", None, true)]);
    }

    // --- the connection ---------------------------------------------------

    use super::wire::fake::{Server, Step};

    /// A session of `kind` over a scripted server, one script a connection.
    fn session_of(kind: GraphKind, scripts: Vec<Vec<Step>>) -> (GraphClient, Server) {
        let server = Server::new(scripts);
        let client = GraphClient::over(
            kind,
            &SecretString::from("token"),
            "me",
            server.connect(),
            SizeHints::default(),
            Arc::new(Throttle::new("msgraph")),
        )
        .unwrap();
        (client, server)
    }

    fn session(scripts: Vec<Vec<Step>>) -> (GraphClient, Server) {
        session_of(GraphKind::Mail, scripts)
    }

    fn inbox() -> Step {
        Step::ok(serde_json::json!({ "id": "id-inbox", "displayName": "Inbox" }))
    }

    fn get_inbox(client: &mut GraphClient) -> Result<MsgraphMailFolder, MsgraphClientStdError> {
        client.op(|client| client.mail_folder_get("inbox"))
    }

    /// The request lines of every connection, methods and paths only.
    fn lines(server: &Server) -> Vec<Vec<String>> {
        server
            .requests()
            .into_iter()
            .map(|requests| {
                requests
                    .into_iter()
                    .map(|line| line.split('?').next().unwrap_or_default().to_owned())
                    .collect()
            })
            .collect()
    }

    const GET_INBOX: &str = "GET /v1.0/me/mailFolders/inbox";

    /// Graph closed the connection while it idled: a `GET` meeting its end
    /// of stream, or a reset, before any byte of an answer goes again once,
    /// on a new connection.
    #[test]
    fn a_get_on_a_connection_closed_while_it_idled_is_sent_again_once() {
        for dead in [Step::Close, Step::Reset] {
            let (mut client, server) = session(vec![vec![inbox(), dead], vec![inbox()]]);

            get_inbox(&mut client).unwrap();
            assert_eq!(get_inbox(&mut client).unwrap().id, "id-inbox");
            assert_eq!(
                lines(&server),
                [vec![GET_INBOX, GET_INBOX], vec![GET_INBOX]],
                "sent again on a second connection"
            );
        }
    }

    /// Sent again once only: a second dead connection fails the request.
    #[test]
    fn a_get_is_sent_again_once_only() {
        let (mut client, server) = session(vec![
            vec![inbox(), Step::Close],
            vec![Step::Close],
            vec![inbox()],
        ]);

        get_inbox(&mut client).unwrap();
        assert!(get_inbox(&mut client).is_err());
        assert_eq!(lines(&server).len(), 2);

        // NOTE: the failed connection is not reused.
        get_inbox(&mut client).unwrap();
        assert_eq!(lines(&server).len(), 3);
    }

    /// A request that writes (a move is a `POST`) is never sent again, even
    /// on a connection that died before answering: it may have landed.
    #[test]
    fn a_post_on_a_dead_connection_is_not_sent_again() {
        let (mut client, server) = session(vec![vec![inbox(), Step::Close], vec![inbox()]]);
        client
            .folders
            .insert(String::from("Archive"), String::from("id-archive"));

        get_inbox(&mut client).unwrap();
        let err = client.move_messages("Archive", &["m1"]).unwrap_err();
        assert!(format!("{err:#}").contains("Move message m1"), "{err:#}");
        assert_eq!(lines(&server)[0][1], "POST /v1.0/me/messages/m1/move");
        assert_eq!(lines(&server).len(), 1, "no second connection");

        get_inbox(&mut client).unwrap();
        assert_eq!(lines(&server)[1], [GET_INBOX], "the next request reopens");
    }

    /// A JSON batch of gets reads only, so it goes again like a `GET`.
    #[test]
    fn a_batch_of_gets_on_a_dead_connection_is_sent_again() {
        let answer = serde_json::json!({ "responses": [
            { "id": "0", "status": 200, "body": STANDARD.encode(b"Subject: a\r\n\r\nA") },
        ]});
        let (mut client, server) =
            session(vec![vec![inbox(), Step::Reset], vec![Step::ok(answer)]]);

        get_inbox(&mut client).unwrap();
        client.sizes.note([(String::from("m0"), 15)]);
        let mut bodies = Vec::new();
        client
            .fetch_messages(
                "Inbox",
                &["m0"],
                &|_| None,
                |_| Ok(Vec::new()),
                |id, _, sink: Vec<u8>| {
                    bodies.push((id.to_owned(), sink));
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(
            bodies,
            [(String::from("m0"), b"Subject: a\r\n\r\nA".to_vec())]
        );
        assert_eq!(lines(&server)[1], ["POST /v1.0/$batch"]);
    }

    /// A read that timed out may be a server still at work on the request:
    /// it is not sent again, and the connection, whose answer may still
    /// come, is not reused.
    #[test]
    fn a_timed_out_read_is_not_sent_again_and_its_connection_is_dropped() {
        let (mut client, server) =
            session(vec![vec![inbox(), Step::Timeout, inbox()], vec![inbox()]]);

        get_inbox(&mut client).unwrap();
        let err = get_inbox(&mut client).unwrap_err();
        assert!(wire::timed_out(&err), "{err}");
        assert_eq!(lines(&server).len(), 1, "not sent again");

        get_inbox(&mut client).unwrap();
        assert_eq!(
            lines(&server),
            [vec![GET_INBOX, GET_INBOX], vec![GET_INBOX]],
            "the late answer is never read as the next request's"
        );
    }

    /// The first request of a connection that ends gets the server's
    /// answer: a fresh connection is not one that idled.
    #[test]
    fn a_fresh_connection_that_ends_is_not_tried_again() {
        let (mut client, server) = session(vec![vec![Step::Close], vec![inbox()]]);

        assert!(get_inbox(&mut client).is_err());
        assert_eq!(lines(&server).len(), 1);
    }

    /// A connection idle past a minute is dropped before the next request,
    /// below the two minutes after which Graph closes it.
    #[test]
    fn an_idle_connection_is_not_reused() {
        let (mut client, server) = session(vec![vec![inbox(), inbox()], vec![inbox()]]);

        get_inbox(&mut client).unwrap();
        client.used = Instant::now()
            .checked_sub(wire::IDLE + Duration::from_secs(1))
            .unwrap();
        get_inbox(&mut client).unwrap();

        assert_eq!(lines(&server), [vec![GET_INBOX], vec![GET_INBOX]]);
    }

    /// A refusal is a whole answer, which keeps the connection; an answer
    /// read to the end of the stream, or one closing the connection,
    /// does not.
    #[test]
    fn a_connection_is_reused_only_after_a_whole_answer() {
        let (mut client, server) = session(vec![
            vec![
                Step::error(404, "ErrorItemNotFound"),
                inbox(),
                Step::Reply(b"HTTP/1.1 200 OK\r\n\r\n{\"id\":\"x\"}".to_vec()),
            ],
            vec![Step::Reply(
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 10\r\n\r\n{\"id\":\"x\"}"
                    .to_vec(),
            )],
            vec![inbox()],
        ]);

        assert!(get_inbox(&mut client).is_err());
        get_inbox(&mut client).unwrap();
        get_inbox(&mut client).unwrap();
        get_inbox(&mut client).unwrap();
        get_inbox(&mut client).unwrap();

        assert_eq!(
            lines(&server).iter().map(Vec::len).collect::<Vec<_>>(),
            [3, 1, 1]
        );
    }

    // --- a write on an item gone converges --------------------------------

    /// A flag set, a move or a delete on a message gone from the server
    /// counts as done; a copy of it stays an error.
    #[test]
    fn a_write_on_a_message_gone_counts_as_done_but_a_copy() {
        let gone = || Step::error(404, "ErrorItemNotFound");
        let (mut client, server) = session(vec![vec![gone(), gone(), gone(), gone(), gone()]]);
        client
            .folders
            .insert(String::from("Archive"), String::from("id-archive"));

        client
            .store_flags(&["m1"], &[Flag::from_iana(IanaFlag::Seen)], FlagOp::Set)
            .unwrap();
        client.move_messages("Archive", &["m2"]).unwrap();
        client.delete_item("m3", None).unwrap();
        let Err(err) = client.copy_message("Archive", "m4") else {
            panic!("a copy of a message gone is an error");
        };
        assert!(format!("{err:#}").contains("404"), "{err:#}");

        assert_eq!(
            lines(&server)[0],
            [
                "PATCH /v1.0/me/messages/m1",
                "POST /v1.0/me/messages/m2/move",
                "GET /v1.0/me/messages/m2",
                "DELETE /v1.0/me/messages/m3",
                "POST /v1.0/me/messages/m4/copy",
            ]
        );
    }

    /// A move answered 404 while the message is still there names a
    /// destination Graph no longer knows: an error.
    #[test]
    fn a_move_to_a_folder_gone_stays_an_error() {
        let (mut client, _) = session(vec![vec![
            Step::error(404, "ErrorItemNotFound"),
            Step::ok(serde_json::json!({ "id": "m1" })),
        ]]);
        client
            .folders
            .insert(String::from("Archive"), String::from("id-archive"));

        let err = client.move_messages("Archive", &["m1"]).unwrap_err();
        assert!(format!("{err:#}").contains("Move message m1"), "{err:#}");
    }

    /// A contact or an event gone from the server is deleted already,
    /// whether the delete or the read checking its revision meets the 404.
    #[test]
    fn a_contact_or_an_event_gone_counts_as_deleted() {
        for kind in [GraphKind::Contacts, GraphKind::Calendar] {
            let gone = || Step::error(404, "ErrorItemNotFound");
            let (mut client, server) = session_of(kind, vec![vec![gone(), gone()]]);

            client.delete_item("c1", None).unwrap();
            client.delete_item("c2", Some("ck-1")).unwrap();
            assert_eq!(lines(&server)[0].len(), 2);
            assert!(lines(&server)[0][0].starts_with("DELETE "));
            assert!(lines(&server)[0][1].starts_with("GET "));
        }
    }

    // --- bodies by batches bounded in bytes -------------------------------

    /// Batches hold at most twenty gets and 4 MiB stated, in the order
    /// asked; a message over 1 MiB, or of no stated size, is read alone.
    #[test]
    fn bodies_are_planned_by_their_stated_sizes() {
        let kib = 1024;
        let sizes: HashMap<&str, u64> = [
            ("a", 900 * kib),
            ("b", 900 * kib),
            ("c", 900 * kib),
            ("d", 900 * kib),
            ("e", 900 * kib),
            ("big", 2 * 1024 * kib),
            ("f", 10 * kib),
        ]
        .into_iter()
        .collect();
        let ids = ["a", "b", "c", "d", "big", "e", "unknown", "f"];

        let planned = plan(&ids, |id| sizes.get(id).copied());
        assert_eq!(
            planned,
            [
                Fetch::Alone("big"),
                Fetch::Batch(vec!["a", "b", "c", "d"]),
                Fetch::Alone("unknown"),
                Fetch::Batch(vec!["e", "f"]),
            ]
        );

        let small: Vec<String> = (0..45).map(|n| format!("m{n}")).collect();
        let small: Vec<&str> = small.iter().map(String::as_str).collect();
        let planned = plan(&small, |_| Some(kib));
        let lens: Vec<usize> = planned
            .iter()
            .map(|fetch| match fetch {
                Fetch::Batch(batch) => batch.len(),
                Fetch::Alone(_) => 0,
            })
            .collect();
        assert_eq!(lens, [20, 20, 5]);

        for fetch in plan(&ids, |id| sizes.get(id).copied()) {
            if let Fetch::Batch(batch) = fetch {
                let stated: u64 = batch.iter().map(|id| sizes[id]).sum();
                assert!(
                    stated <= BATCH_BYTES,
                    "a batch holds {stated} stated octets"
                );
            }
        }
    }

    fn raw_reply(raw: &[u8]) -> Step {
        let mut reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n",
            raw.len()
        )
        .into_bytes();
        reply.extend_from_slice(raw);
        Step::Reply(reply)
    }

    fn size_row(id: &str, size: u64) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "singleValueExtendedProperties": [{ "id": "Integer 0xe08", "value": size.to_string() }],
        })
    }

    /// The sizes the listing stated plan the batches; a lone message of no
    /// stated size is read alone, without asking its size, as one past a
    /// mebibyte, where it comes; every body is committed byte for byte.
    #[test]
    fn bodies_follow_the_sizes_the_listing_stated() {
        let body = |id: &str| format!("Subject: {id}\r\n\r\nbody of {id}").into_bytes();
        let answer = serde_json::json!({ "responses": [
            { "id": "1", "status": 200, "body": STANDARD.encode(body("m1")) },
            { "id": "0", "status": 200, "body": STANDARD.encode(body("m0")) },
        ]});
        let (mut client, server) = session(vec![vec![
            raw_reply(&body("big")),
            raw_reply(&body("m2")),
            Step::ok(answer),
        ]]);
        client.sizes.note([
            (String::from("m0"), 2_000),
            (String::from("big"), 5_000_000),
            (String::from("m1"), 3_000),
        ]);

        let mut bodies = Vec::new();
        client
            .fetch_messages(
                "Inbox",
                &["m0", "big", "m1", "m2"],
                &|_| None,
                |_| Ok(Vec::new()),
                |id, _, sink: Vec<u8>| {
                    bodies.push((id.to_owned(), sink));
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(
            bodies,
            [
                (String::from("big"), body("big")),
                (String::from("m2"), body("m2")),
                (String::from("m1"), body("m1")),
                (String::from("m0"), body("m0")),
            ]
        );
        assert_eq!(
            lines(&server)[0],
            [
                "GET /v1.0/me/messages/big/$value",
                "GET /v1.0/me/messages/m2/$value",
                "POST /v1.0/$batch",
            ]
        );
        assert!(client.sizes.take("m0").is_none(), "a size serves once");
    }

    /// Messages of no stated size are asked theirs first, twenty a batch,
    /// with the MAPI size expanded; one Graph states none for is read alone.
    #[test]
    fn sizes_unstated_are_asked_before_the_bodies() {
        let body = |id: &str| format!("Subject: {id}\r\n\r\n{id}").into_bytes();
        let sizes = serde_json::json!({ "responses": [
            { "id": "0", "status": 200, "body": size_row("m0", 100) },
            { "id": "1", "status": 200, "body": size_row("m1", 3_000_000) },
            { "id": "2", "status": 200, "body": { "id": "m2" } },
            { "id": "3", "status": 200, "body": size_row("m3", 200) },
        ]});
        let bodies = serde_json::json!({ "responses": [
            { "id": "0", "status": 200, "body": STANDARD.encode(body("m0")) },
            { "id": "1", "status": 200, "body": STANDARD.encode(body("m3")) },
        ]});
        let (mut client, server) = session(vec![vec![
            Step::ok(sizes),
            raw_reply(&body("m1")),
            raw_reply(&body("m2")),
            Step::ok(bodies),
        ]]);

        let mut fetched = Vec::new();
        client
            .fetch_messages(
                "Inbox",
                &["m0", "m1", "m2", "m3"],
                &|_| None,
                |_| Ok(Vec::new()),
                |id, _, _: Vec<u8>| {
                    fetched.push(id.to_owned());
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(fetched, ["m1", "m2", "m0", "m3"]);
        assert_eq!(
            lines(&server)[0],
            [
                "POST /v1.0/$batch",
                "GET /v1.0/me/messages/m1/$value",
                "GET /v1.0/me/messages/m2/$value",
                "POST /v1.0/$batch",
            ]
        );
        let written = server.written(0);
        assert!(
            written.contains(
                "/me/messages/m0?%24select=id&%24expand=singleValueExtendedProperties%28%24filter%3Did+eq+%27Integer+0x0E08%27%29"
            ),
            "{written}"
        );
    }

    /// The sizes the store records plan the batches of a resumed
    /// hydration: no size is asked first.
    #[test]
    fn the_sizes_the_store_records_spare_the_size_batch() {
        let body = |id: &str| format!("Subject: {id}\r\n\r\n{id}").into_bytes();
        let bodies = serde_json::json!({ "responses": [
            { "id": "0", "status": 200, "body": STANDARD.encode(body("m0")) },
            { "id": "1", "status": 200, "body": STANDARD.encode(body("m1")) },
        ]});
        let (mut client, server) = session(vec![vec![Step::ok(bodies)]]);

        let stored = |id: &str| Some(if id == "m0" { 100 } else { 200 });
        let mut fetched = Vec::new();
        let gone = client
            .fetch_messages(
                "Inbox",
                &["m0", "m1"],
                &stored,
                |_| Ok(Vec::new()),
                |id, _, _: Vec<u8>| {
                    fetched.push(id.to_owned());
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(fetched, ["m0", "m1"]);
        assert!(gone.is_empty());
        assert_eq!(lines(&server)[0], ["POST /v1.0/$batch"], "the bodies alone");
    }

    /// A message Graph answers 404, asked its size, in a batch or alone,
    /// is answered gone, and the fetch goes on.
    #[test]
    fn a_message_gone_is_answered_gone() {
        let body = |id: &str| format!("Subject: {id}\r\n\r\n{id}").into_bytes();
        let missing = || serde_json::json!({ "error": { "code": "ErrorItemNotFound" } });
        let sizes = serde_json::json!({ "responses": [
            { "id": "0", "status": 404, "body": missing() },
            { "id": "1", "status": 200, "body": size_row("m3", 100) },
        ]});
        let bodies = serde_json::json!({ "responses": [
            { "id": "0", "status": 200, "body": STANDARD.encode(body("m3")) },
            { "id": "1", "status": 404, "body": missing() },
            { "id": "2", "status": 200, "body": STANDARD.encode(body("m2")) },
        ]});
        let (mut client, server) = session(vec![vec![
            Step::ok(sizes),
            Step::error(404, "ErrorItemNotFound"),
            Step::ok(bodies),
        ]]);

        // NOTE: m0 and m3 state no size, so theirs are asked first; m0 is
        // gone there, m1 in the body batch, m4 read alone (past 1 MiB).
        let stored = |id: &str| match id {
            "m1" | "m2" => Some(100),
            "m4" => Some(2 * 1024 * 1024),
            _ => None,
        };
        let mut fetched = Vec::new();
        let gone = client
            .fetch_messages(
                "Inbox",
                &["m0", "m3", "m1", "m2", "m4"],
                &stored,
                |_| Ok(Vec::new()),
                |id, _, _: Vec<u8>| {
                    fetched.push(id.to_owned());
                    Ok(())
                },
            )
            .unwrap();

        let mut gone = gone;
        gone.sort();
        assert_eq!(gone, ["m0", "m1", "m4"]);
        assert_eq!(fetched, ["m3", "m2"]);
        let mut asked = lines(&server)[0].clone();
        asked.sort();
        assert_eq!(
            asked,
            [
                "GET /v1.0/me/messages/m4/$value",
                "POST /v1.0/$batch",
                "POST /v1.0/$batch",
            ]
        );
    }

    /// A message read alone and answered 404 is [`Gone`]; another refusal
    /// is not.
    #[test]
    fn a_message_read_alone_and_gone_is_gone() {
        let (mut client, _server) = session(vec![vec![
            Step::error(404, "ErrorItemNotFound"),
            Step::error(403, "ErrorAccessDenied"),
        ]]);
        let err = client
            .get_item_stream("Inbox", "m1", Vec::new())
            .unwrap_err();
        assert!(Gone::is(&err), "{err:#}");
        let err = client
            .get_item_stream("Inbox", "m1", Vec::new())
            .unwrap_err();
        assert!(!Gone::is(&err), "{err:#}");
    }

    /// Each throttled get of a batch waits its own `Retry-After`: one asking
    /// for no wait goes in the next batch at once, one asking for a second
    /// in the batch after, alone.
    #[test]
    fn a_throttled_get_waits_its_own_retry_after() {
        let first = serde_json::json!({ "responses": [
            { "id": "0", "status": 200, "body": STANDARD.encode(b"Subject: 0\r\n\r\n0") },
            { "id": "1", "status": 429, "headers": { "Retry-After": "0" } },
            { "id": "2", "status": 503, "headers": { "Retry-After": "1" } },
        ]});
        let one = |text: &[u8]| {
            Step::ok(serde_json::json!({ "responses": [
                { "id": "0", "status": 200, "body": STANDARD.encode(text) },
            ]}))
        };
        let (mut client, server) = session(vec![vec![
            Step::ok(first),
            one(b"Subject: 1\r\n\r\n1"),
            one(b"Subject: 2\r\n\r\n2"),
        ]]);

        let start = Instant::now();
        let mut delivered = Vec::new();
        let answer = client
            .batch_get(
                &["m0", "m1", "m2"],
                |id| format!("/me/messages/{id}/$value"),
                raw_body,
                |id, _| {
                    delivered.push((id, start.elapsed()));
                    Ok(())
                },
            )
            .unwrap();

        let ids: Vec<&str> = delivered.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, ["m0", "m1", "m2"]);
        assert!(
            delivered[1].1 < Duration::from_millis(900),
            "m1 did not wait for m2"
        );
        assert!(
            delivered[2].1 >= Duration::from_secs(1),
            "m2 waited its own second"
        );
        assert!(answer.retry.is_empty());
        assert!(client.throttle.gave_up().is_none());

        let written = server.written(0);
        let batches: Vec<&str> = written.split("POST /v1.0/$batch").skip(1).collect();
        assert_eq!(batches.len(), 3);
        assert!(batches[1].contains("/me/messages/m1/$value"));
        assert!(!batches[1].contains("/me/messages/m2/$value"));
        assert!(batches[2].contains("/me/messages/m2/$value"));
    }

    /// A get asking to wait longer than neverest does is not sent again:
    /// the source gives up until the wait is over, and every get left goes
    /// to the next run.
    #[test]
    fn a_throttled_get_asking_too_long_a_wait_gives_the_source_up() {
        let first = serde_json::json!({ "responses": [
            { "id": "0", "status": 200, "body": STANDARD.encode(b"Subject: 0\r\n\r\n0") },
            { "id": "1", "status": 429, "headers": { "Retry-After": "0" } },
            { "id": "2", "status": 429, "headers": { "Retry-After": "3600" } },
        ]});
        let (mut client, server) = session(vec![vec![Step::ok(first)]]);

        let mut delivered = Vec::new();
        let answer = client
            .batch_get(
                &["m0", "m1", "m2"],
                |id| format!("/me/messages/{id}/$value"),
                raw_body,
                |id, _| {
                    delivered.push(id);
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(delivered, ["m0"]);
        assert_eq!(answer.retry, [(1, "m1"), (2, "m2")]);
        let until = client.throttle.gave_up().expect("the source gave up");
        assert!(until > SystemTime::now() + Duration::from_secs(3000));
        assert_eq!(lines(&server)[0].len(), 1, "nothing sent past the give-up");
    }

    /// The octets an answer held while it was read are measured: its base64
    /// bodies, before any is decoded.
    #[test]
    fn a_batch_answer_measures_what_it_held() {
        let raw = vec![b'x'; 3_000];
        let encoded = STANDARD.encode(&raw);
        let responses: MsgraphBatchResponses = serde_json::from_value(serde_json::json!({
            "responses": [
                { "id": "0", "status": 200, "body": encoded },
                { "id": "1", "status": 200, "body": encoded },
            ]
        }))
        .unwrap();

        let (bodies, answer) = read_raw_responses(&["m0", "m1"], responses);
        assert_eq!(bodies.len(), 2);
        assert_eq!(answer.held, 2 * encoded.len());
    }

    /// A summary read by id expands the MAPI size, which the envelope
    /// carries.
    #[test]
    fn the_envelope_carries_the_expanded_size() {
        let message: MsgraphMessage = serde_json::from_value(size_row("m1", 48_213)).unwrap();
        assert_eq!(message_envelope("m1", &message).size, 48_213);
        assert_eq!(message_envelope("m1", &fixture_row()).size, 0);

        assert_eq!(
            batch_url(
                "me/messages/AAMk-1=",
                &[("$select", "id"), ("$expand", SIZE_EXPAND)]
            ),
            "/me/messages/AAMk-1=?%24select=id&%24expand=singleValueExtendedProperties%28%24filter%3Did+eq+%27Integer+0x0E08%27%29"
        );
    }

    /// A request whose write meets a connection the server closed (a broken
    /// pipe) never reached it: a `GET` goes again on a new connection.
    #[test]
    fn a_get_whose_write_breaks_on_a_closed_connection_is_sent_again() {
        let (mut client, server) = session(vec![vec![inbox(), Step::Broken], vec![inbox()]]);

        get_inbox(&mut client).unwrap();
        get_inbox(&mut client).unwrap();
        assert_eq!(lines(&server), [vec![GET_INBOX], vec![GET_INBOX]]);
    }

    /// A whole-folder delta asks for the size `$expand`; refused, it asks
    /// again without it, and the session no longer asks for it.
    #[test]
    fn a_delta_refusing_the_size_expand_is_asked_without_it() {
        let page = || {
            Step::ok(serde_json::json!({
                "value": [],
                "@odata.deltaLink": "https://graph.microsoft.com/v1.0/me/mailFolders/id-inbox/messages/delta?$deltatoken=a",
            }))
        };
        let (mut client, server) =
            session(vec![vec![Step::error(400, "BadRequest"), page(), page()]]);
        client
            .folders
            .insert(String::from("Inbox"), String::from("id-inbox"));

        use mail::MailWire;
        client.delta_first("Inbox", mail::SUMMARY_SELECT).unwrap();
        client.delta_first("Inbox", mail::SUMMARY_SELECT).unwrap();

        let requests = server.requests().remove(0);
        assert_eq!(requests.len(), 3);
        assert!(requests[0].contains("%24expand="), "{}", requests[0]);
        assert!(!requests[1].contains("%24expand="), "{}", requests[1]);
        assert!(!requests[2].contains("%24expand="), "{}", requests[2]);
    }

    const SEND_MAIL: &str = "POST /v1.0/me/sendMail";

    fn accepted() -> Step {
        Step::Reply(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n".to_vec())
    }

    fn send_mail(client: &mut GraphClient) -> Result<(), SendMimeFailure> {
        client.send_mime(b"Subject: hi\r\n\r\nhi")
    }

    /// A send answered, whatever the status, carries its verdict.
    #[test]
    fn a_send_answered_is_settled_whatever_the_status() {
        let (mut client, _) = session(vec![vec![accepted()]]);
        send_mail(&mut client).unwrap();

        let (mut client, server) = session(vec![vec![Step::error(503, "ServiceUnavailable")]]);
        let failure = send_mail(&mut client).unwrap_err();
        let MsgraphClientStdError::Send(err) = &failure.error else {
            panic!("a 503 is an API error: {failure:?}");
        };
        assert_eq!(err.status(), Some(503));
        assert!(!failure.unanswered, "a status answers the request");
        assert_eq!(lines(&server), [vec![SEND_MAIL]], "a 503 is not sent again");
    }

    /// A send written whole on a connection that then ends, with no answer
    /// or with one cut short, may have been taken: never sent again, and
    /// told unanswered.
    #[test]
    fn a_send_written_whole_and_not_answered_whole_is_unanswered() {
        let cut = Step::Reply(b"HTTP/1.1 202 Acc".to_vec());
        for (dead, after) in [
            (Step::Close, vec![]),
            (Step::Reset, vec![]),
            (Step::Timeout, vec![]),
            (cut, vec![Step::Close]),
        ] {
            let mut script = vec![inbox(), dead];
            script.extend(after);
            let (mut client, server) = session(vec![script, vec![accepted()]]);

            get_inbox(&mut client).unwrap();
            let failure = send_mail(&mut client).unwrap_err();
            assert!(failure.unanswered, "{failure:?}");
            assert_eq!(
                lines(&server),
                [vec![GET_INBOX, SEND_MAIL]],
                "a send is never sent again on a new connection"
            );
        }
    }

    /// A send whose request could not be written went nowhere.
    #[test]
    fn a_send_not_written_whole_is_answered_as_never_sent() {
        let (mut client, _) = session(vec![vec![inbox(), Step::Broken], vec![accepted()]]);

        get_inbox(&mut client).unwrap();
        let failure = send_mail(&mut client).unwrap_err();
        assert!(!failure.unanswered, "{failure:?}");
    }
}
