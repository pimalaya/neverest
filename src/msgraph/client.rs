//! # Microsoft Graph client
//!
//! [`GraphClient`] wraps the std blocking io-msgraph client behind the same
//! adapter surface as the IMAP backend, with the folder name map, the delta row
//! cache serving the `Meta` tier and stream reopens.
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

use std::{
    collections::{BTreeMap, HashMap},
    io::{Read, Write},
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, bail};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD},
};
use chrono::{DateTime, FixedOffset};
use io_msgraph::v1::{
    client::{MsgraphClientStd, MsgraphClientStdConnectOptions, MsgraphClientStdError},
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

use crate::{
    client::{Held, Listed, WrittenItem},
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
    /// The TLS configuration, kept for stream reopens.
    tls: Tls,
    /// Folder display name (or `Parent/Child` path) to folder id, refreshed
    /// from the folder listing when a name misses.
    folders: HashMap<String, String>,
    /// The delta rows of the last enumerations, keyed by collection then
    /// handle, serving the `Meta` tier without re-fetching.
    rows: HashMap<String, HashMap<String, MsgraphMessage>>,
    /// The id of the well-known Drafts folder, resolved on the first append.
    drafts: Option<String>,
    /// Whether the server allowed reusing the stream after the last exchange;
    /// when false the next operation reopens it.
    alive: bool,
    /// The source's back-off, shared by its connections.
    throttle: Arc<Throttle>,
}

impl GraphClient {
    /// Opens the TLS connection to the Graph API with the given bearer token,
    /// scoped to the `user` mailbox owner (`me` or a user id), for one kind.
    pub fn connect(
        kind: GraphKind,
        token: &SecretString,
        user: &str,
        tls: Tls,
        throttle: Arc<Throttle>,
    ) -> Result<Self> {
        let options = MsgraphClientStdConnectOptions {
            tls: tls.clone(),
            proxy: Proxy::None,
            user_id: user.to_owned(),
        };
        let inner = MsgraphClientStd::connect(token.expose_secret(), options)
            .context("Cannot connect to Microsoft Graph")?;

        Ok(Self {
            kind,
            inner,
            tls,
            folders: HashMap::new(),
            rows: HashMap::new(),
            drafts: None,
            alive: true,
            throttle,
        })
    }

    /// Reopens the stream to the Graph API endpoint, keeping the credential.
    fn reconnect(&mut self) -> Result<()> {
        debug!("reopening the graph stream");

        let url = Url::parse(MSGRAPH_API_BASE).context("Cannot parse the Graph API base URL")?;
        let host = url.host_str().context("Graph API base URL has no host")?;
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

    /// Runs one Graph operation, reopening the stream first when the server
    /// closed it, and records the new keep-alive hint.
    ///
    /// The source's [`Throttle`] sends it again while Graph throttles it
    /// (429, 503), backing off: io-msgraph keeps no `Retry-After` but in a
    /// batch's answers. A request creating something goes through
    /// [`create`](Self::create) instead.
    fn op<T>(
        &mut self,
        run: impl FnMut(&mut MsgraphClientStd) -> Result<MsgraphSendOutput<T>, MsgraphClientStdError>,
    ) -> Result<T, MsgraphClientStdError> {
        self.send(Request::Idempotent, run)
    }

    /// Runs one Graph request that creates or sends something: sent again on
    /// a 429 alone, which Graph answers before doing anything, never on a
    /// 503, after which the message, the copy or the event may exist.
    fn create<T>(
        &mut self,
        run: impl FnMut(&mut MsgraphClientStd) -> Result<MsgraphSendOutput<T>, MsgraphClientStdError>,
    ) -> Result<T, MsgraphClientStdError> {
        self.send(Request::Create, run)
    }

    /// Runs one Graph request of `request`, as [`op`](Self::op) describes.
    fn send<T>(
        &mut self,
        request: Request,
        mut run: impl FnMut(
            &mut MsgraphClientStd,
        ) -> Result<MsgraphSendOutput<T>, MsgraphClientStdError>,
    ) -> Result<T, MsgraphClientStdError> {
        let throttle = Arc::clone(&self.throttle);
        throttle.call(
            1,
            || {
                if !self.alive {
                    self.reconnect().map_err(MsgraphClientStdError::Tls)?;
                }

                let out = run(&mut self.inner)?;
                self.alive = out.keep_alive;
                Ok(out.response)
            },
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
                // NOTE: the row cache is taken out for the listing, which
                // borrows the session as its wire.
                let mut rows = self.rows.remove(collection).unwrap_or_default();
                let listed = mail::enumerate(self, &mut rows, collection, request, held);
                self.rows.insert(collection.to_owned(), rows);
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
    /// item, `done` receiving the revision on contacts.
    pub fn fetch_bodies<S: Write>(
        &mut self,
        collection: &str,
        ids: &[&str],
        open: impl FnMut(&str) -> std::io::Result<S>,
        done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<()> {
        match self.kind {
            GraphKind::Mail => self.fetch_messages(collection, ids, open, done),
            GraphKind::Contacts => self.fetch_contacts(ids, open, done),
            GraphKind::Calendar => self.fetch_events(ids, open, done),
        }
    }

    /// Streams one item into `sink`, returning its revision on contacts.
    pub fn get_item_stream(
        &mut self,
        collection: &str,
        id: &str,
        sink: impl Write,
    ) -> Result<Option<String>> {
        match self.kind {
            GraphKind::Mail => self.get_message_stream(collection, id, sink).map(|()| None),
            GraphKind::Contacts => self.get_contact_stream(id, sink),
            GraphKind::Calendar => self.get_event_stream(id, sink),
        }
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

    /// The delta row of a handle: the enumeration cache when it holds it, else
    /// a targeted single-message get.
    fn row(&mut self, mailbox: &str, id: &str) -> Result<MsgraphMessage> {
        match self.rows.get(mailbox).and_then(|cache| cache.get(id)) {
            Some(message) => Ok(message.clone()),
            None => self
                .op(|client| client.message_get(id))
                .with_context(|| format!("Get message {id} error")),
        }
    }

    /// Fetches envelopes for a message-id set, served from the cached delta
    /// rows (one message get per handle missing the cache).
    ///
    /// Graph exposes no RFC 5322 octet size, so `size` stays 0 and is filled
    /// from the blob length at the `Full` tier.
    fn fetch_envelopes(&mut self, mailbox: &str, ids: &[&str]) -> Result<Vec<ItemSummary>> {
        let mut envelopes = Vec::with_capacity(ids.len());
        for id in ids {
            let message = self.row(mailbox, id)?;
            envelopes.push(message_envelope(id, &message));
        }
        Ok(envelopes)
    }

    /// Streams the bodies of a message-id set through JSON batches: up to
    /// [`MSGRAPH_BATCH_MAX_REQUESTS`] raw MIME gets per HTTP call, each body
    /// answered base64-encoded, committed in request order.
    ///
    /// A body still missing after [`batch_get`](Self::batch_get)'s retries,
    /// or refused for another reason, is left out: the caller fetches every
    /// handle a batch did not answer one by one, which surfaces its error.
    fn fetch_messages<S: Write>(
        &mut self,
        _mailbox: &str,
        ids: &[&str],
        mut open: impl FnMut(&str) -> std::io::Result<S>,
        mut done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<()> {
        let user = user_path(&self.inner.user_id);

        for chunk in ids.chunks(MSGRAPH_BATCH_MAX_REQUESTS) {
            let mut answer = self
                .batch_get(
                    chunk,
                    |id| format!("/{user}/messages/{id}/$value"),
                    raw_body,
                )
                .context("Send raw message batch error")?;

            for id in chunk {
                let Some(raw) = answer.bodies.remove(id) else {
                    continue;
                };
                let mut sink =
                    open(id).with_context(|| format!("Open body sink for {id} error"))?;
                sink.write_all(&raw)
                    .with_context(|| format!("Store body {id} error"))?;
                done(id, None, sink).with_context(|| format!("Commit body {id} error"))?;
            }
        }

        Ok(())
    }

    /// Sends the gets of `ids` (at most [`MSGRAPH_BATCH_MAX_REQUESTS`]) in
    /// one JSON batch, each at the URL `url` builds for its id, its body
    /// read by `decode`.
    ///
    /// A request Graph throttles (429, or a common 5xx) is sent again in the
    /// next batch after the longest `Retry-After` it stated, at most
    /// [`BATCH_RETRY_ROUNDS`] times; still throttled then, it is answered
    /// under `retry`, and a throttle (429, 503) gives the source up. A 404 is
    /// answered under `gone`; any other refusal is left out.
    fn batch_get<'a, T>(
        &mut self,
        ids: &[&'a str],
        url: impl Fn(&str) -> String,
        decode: impl Fn(&str, Option<serde_json::Value>) -> Option<T>,
    ) -> Result<BatchAnswer<'a, T>, MsgraphClientStdError> {
        let mut out = BatchAnswer::default();
        let mut pending: Vec<&str> = ids.to_vec();

        for round in 0..=BATCH_RETRY_ROUNDS {
            // NOTE: a source that gave up sends nothing until its wait is
            // over, what is left going to the next run.
            if pending.is_empty() || self.throttle.blocked().is_some() {
                break;
            }

            let requests = get_requests(&pending, &url);
            let answered = match self.op(|client| client.batch(&requests)) {
                Ok(responses) => read_responses(&pending, responses, &decode),
                Err(err) if is_throttled(&err) => {
                    let throttled = matches!(
                        &err,
                        MsgraphClientStdError::Send(send) if matches!(send.status(), Some(429 | 503))
                    );
                    BatchAnswer::retry_all(&pending, throttled)
                }
                Err(err) => return Err(err),
            };

            out.bodies.extend(answered.bodies);
            out.gone.extend(answered.gone);
            pending = answered.retry;

            let wait = answered
                .retry_after
                .unwrap_or_else(|| Duration::from_secs(1 << round));
            // NOTE: a request Graph failed to serve (500, 502, 504) is
            // retried the same, but is no throttle: the source goes on.
            if !pending.is_empty() && round == BATCH_RETRY_ROUNDS && answered.throttled {
                self.throttle.give_up(SystemTime::now() + wait);
            }
            if !pending.is_empty() && round < BATCH_RETRY_ROUNDS {
                let wait = wait.min(BATCH_RETRY_MAX_WAIT);
                warn!(
                    "graph throttled {} of {} batched requests, retrying in {}s",
                    pending.len(),
                    ids.len(),
                    wait.as_secs()
                );
                std::thread::sleep(wait);
            }
        }

        out.retry = pending;
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
            self.op(|client| client.message_update(id, &patch))
                .with_context(|| format!("Update flags of {id} error"))?;
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
    pub fn move_messages(&mut self, to: &str, ids: &[&str]) -> Result<()> {
        if self.kind != GraphKind::Mail {
            bail!(
                "Graph {} cannot move between folders here (move not supported)",
                self.kind.media_type()
            );
        }

        let folder = self.folder_id(to)?;
        for id in ids {
            let moved = self
                .op(|client| client.message_move(id, &folder))
                .with_context(|| format!("Move message {id} to {to} error"))?;
            debug!("moved message {id} to {to}, now {}", moved.id);
        }
        Ok(())
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

    /// Deletes one message by id.
    fn delete_message(&mut self, id: &str) -> Result<()> {
        self.op(|client| client.message_delete(id))
            .with_context(|| format!("Delete message {id} error"))?;
        Ok(())
    }

    /// Sends raw MIME through sendMail, which files the message in Sent itself.
    ///
    /// The client error comes back unwrapped, so the caller can read the HTTP
    /// status off it. sendMail derives the recipients from the MIME headers
    /// (Bcc included), so envelope recipients beyond the headers are lost.
    pub fn send_mime(&mut self, raw: &[u8]) -> Result<(), MsgraphClientStdError> {
        self.create(|client| client.mail_send_mime(raw))?;
        Ok(())
    }
}

/// How many times a throttled body request is sent again in a later batch.
const BATCH_RETRY_ROUNDS: u32 = 4;

/// The longest wait between two batch rounds, whatever `Retry-After` says.
const BATCH_RETRY_MAX_WAIT: Duration = Duration::from_secs(120);

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
#[cfg(test)]
fn raw_requests(user: &str, ids: &[&str]) -> Vec<MsgraphBatchRequest> {
    get_requests(ids, |id| format!("/{user}/messages/{id}/$value"))
}

/// What a batch of gets answered.
#[derive(Debug, PartialEq)]
struct BatchAnswer<'a, T = Vec<u8>> {
    /// The decoded bodies, by id.
    bodies: HashMap<&'a str, T>,
    /// The ids Graph throttled, to send again, in request order.
    retry: Vec<&'a str>,
    /// The ids Graph answered 404, gone since they were listed.
    gone: Vec<&'a str>,
    /// The longest `Retry-After` the throttled responses stated.
    retry_after: Option<Duration>,
    /// Whether Graph throttled any of `retry` (429, 503), rather than
    /// failing it (500, 502, 504): only a throttle gives the source up.
    throttled: bool,
}

impl<T> Default for BatchAnswer<'_, T> {
    fn default() -> Self {
        Self {
            bodies: HashMap::new(),
            retry: Vec::new(),
            gone: Vec::new(),
            retry_after: None,
            throttled: false,
        }
    }
}

impl<'a, T> BatchAnswer<'a, T> {
    /// A batch throttled as a whole: every request to send again.
    fn retry_all(ids: &[&'a str], throttled: bool) -> Self {
        Self {
            retry: ids.to_vec(),
            throttled,
            ..Default::default()
        }
    }
}

/// Reads the responses of a batch of raw gets sent for `ids`.
#[cfg(test)]
fn read_raw_responses<'a>(ids: &[&'a str], responses: MsgraphBatchResponses) -> BatchAnswer<'a> {
    read_responses(ids, responses, raw_body)
}

/// Reads the responses of a batch of gets sent for `ids`, matched by
/// request id (Graph answers in any order). A 2xx body is read by
/// `decode`; a throttled request is kept to retry, a 404 named gone; any
/// other answer, or a body `decode` refuses, is left out.
fn read_responses<'a, T>(
    ids: &[&'a str],
    responses: MsgraphBatchResponses,
    decode: impl Fn(&str, Option<serde_json::Value>) -> Option<T>,
) -> BatchAnswer<'a, T> {
    let mut answer = BatchAnswer::default();
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
            answer.retry_after = answer.retry_after.max(after);
            answer.throttled |= matches!(response.status, 429 | 503);
            retry.push(index);
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
            answer.bodies.insert(id, body);
        }
    }

    retry.sort_unstable();
    retry.dedup();
    answer.retry = retry.into_iter().map(|index| ids[index]).collect();
    answer
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
    let mut flags = std::collections::BTreeSet::new();
    if message.is_read == Some(true) {
        flags.insert(Flag::from_iana(IanaFlag::Seen));
    }
    let status = message.flag.as_ref().and_then(|flag| flag.flag_status);
    if status == Some(MsgraphFlagStatus::Flagged) {
        flags.insert(Flag::from_iana(IanaFlag::Flagged));
    }
    if message.is_draft == Some(true) {
        flags.insert(Flag::from_iana(IanaFlag::Draft));
    }
    flags
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
        size: 0,
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

        let answer = read_raw_responses(&ids, responses);

        assert_eq!(answer.bodies.len(), 2, "a refused get is left out");
        assert_eq!(answer.bodies["m0"], raw0);
        assert_eq!(answer.bodies["m2"], raw2);
        assert_eq!(answer.retry, ["m3", "m4"], "in request order");
        assert_eq!(answer.gone, ["m1"], "a 404 is gone since it was listed");
        assert_eq!(answer.retry_after, Some(Duration::from_secs(7)));
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

        let answer = read_raw_responses(&ids, responses);

        assert!(answer.bodies.is_empty());
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
            BatchAnswer::<Vec<u8>>::retry_all(&["m0", "m1"], true).retry,
            ["m0", "m1"]
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
        let answer = read_raw_responses(&["m0", "m1"], responses);
        assert_eq!(answer.retry, ["m0", "m1"]);
        assert!(!answer.throttled);

        let responses: MsgraphBatchResponses = serde_json::from_value(serde_json::json!({
            "responses": [
                { "id": "0", "status": 502 },
                { "id": "1", "status": 503 },
            ]
        }))
        .unwrap();
        assert!(read_raw_responses(&["m0", "m1"], responses).throttled);
    }
}
