//! # Gmail client
//!
//! [`GmailClient`] wraps io-gmail's std client behind the adapter surface the
//! sync engine needs, syncing `message/rfc822` over the Gmail REST API.
//!
//! Gmail holds one message carrying a set of labels, which is pimdir's one
//! item filed in several collections. The collections are the user labels
//! plus [`SYSTEM_COLLECTIONS`], keyed and named by the label name, the way
//! IMAP exposes them, so a Gmail endpoint meets an IMAP one on the same
//! account. `UNREAD`, `STARRED` and `IMPORTANT` are flags, never collections,
//! and the categories are left out: they partition `INBOX`.
//!
//! A handle is the Gmail message id, stable across labels, so no handle-space
//! rebuild ever happens. `enumerate` carries the mailbox `historyId` as the
//! checkpoint: a full round lists the label's ids (plus three id-only listings
//! for the flags), a delta round reads `history.list` scoped to the label, and
//! an expired history id (HTTP 404) restarts a full round.
//!
//! Pushes are label changes: a delete removes the collection's label (from
//! `INBOX` it archives), a move swaps two labels, and only a delete from
//! `TRASH` is permanent. An append goes through `messages.import`, whose
//! answered id is checked before it is trusted.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    io::{Read, Write},
    sync::Arc,
    thread,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::DateTime;
use io_gmail::v1::{
    client::{GmailClientStd, GmailClientStdConnectOptions, GmailClientStdError},
    rest::{
        history::{
            GmailHistory, GmailHistoryType,
            list::{GmailHistoryList, GmailHistoryListParams},
        },
        labels::{GmailLabel, GmailLabelType},
        messages::{
            GmailInternalDateSource, GmailMessage, GmailMessageFormat,
            batch_modify::GmailMessagesBatchModify, decode_raw, encode_raw,
            import::GmailMessageImport, list::GmailMessagesListParams,
        },
    },
    send::{GMAIL_API_BASE, GmailSendError, GmailSendOutput},
};
use io_pimdir::{
    collection::PimdirScope,
    remote::{PimdirEnumerate, PimdirListing},
    summary::mail::{addresses, decode, instant, message_ids, meta_attachment},
};
use log::{debug, trace, warn};
use pimalaya_stream::{
    proxy::Proxy,
    stream::{Stream, TlsConnectOptions},
    tls::Tls,
};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::{
    client::{EnumEntry, Enumeration, Held, Listed, WrittenItem},
    item::{
        address::Address,
        collection::Collection,
        flag::{Flag, FlagOp, IanaFlag},
        summary::{ItemSummary, normalize_message_id},
    },
    kind::mail,
    offline::invitation::Refusal,
    throttle::{Request, Throttle, google_throttled_for},
};

/// Gmail system label marking an unread message; its absence means seen.
const UNREAD: &str = "UNREAD";
/// Gmail system label backing the shared `\Flagged` flag.
const STARRED: &str = "STARRED";
/// Gmail system label backing the shared `$Important` flag.
const IMPORTANT: &str = "IMPORTANT";
/// Gmail system label of the spam folder, hidden from other listings.
const SPAM: &str = "SPAM";
/// Gmail system label of the trash, hidden from other listings.
const TRASH: &str = "TRASH";

/// The system labels synced as collections, next to every user label.
const SYSTEM_COLLECTIONS: &[&str] = &["INBOX", "SENT", "DRAFT", SPAM, TRASH];

/// The headers a `Meta`-tier summary reads.
const SUMMARY_HEADERS: &[&str] = &[
    "Message-ID",
    "In-Reply-To",
    "Subject",
    "From",
    "To",
    "Cc",
    "Bcc",
    "Date",
    "Content-Type",
];

/// The history records a delta round reads.
const DELTA_HISTORY: &[GmailHistoryType] = &[
    GmailHistoryType::MessageAdded,
    GmailHistoryType::MessageDeleted,
    GmailHistoryType::LabelAdded,
    GmailHistoryType::LabelRemoved,
];

/// The page size of message and history listings, the API maximum.
const PAGE_SIZE: u32 = 500;

/// The ids one page of a round lists (pimdir SYNC §4): their metadata read
/// near 40 a second commits a page every two and a half seconds.
const ROUND_PAGE_SIZE: u32 = 100;

/// The metadata reads a page makes in a row, Google advising batches of 50
/// at most; io-gmail sends no HTTP batch, so each is its own request, paced.
const META_CHUNK: usize = 50;

/// How far around a scope the `after:` and `before:` searches reach: Gmail
/// files a message by its reception, which may differ from its `Date`.
const SCOPE_MARGIN_DAYS: i64 = 2;

/// The most ids one `batchModify` takes.
const BATCH_SIZE: usize = 1000;

/// How many times an import's history is read before giving up on it.
const IMPORT_ATTEMPTS: u32 = 3;

/// The quota units a second the Gmail source holds itself to, across all its
/// connections.
///
/// Gmail meters each user at 250 units a second, `messages.list` and
/// `messages.get` costing 5 each: about 50 reads a second, batched or not, a
/// batch counting each request it carries. 200 is 40 reads a second, a margin
/// below the quota rather than a run of refusals at it.
pub const UNITS_PER_SECOND: f64 = 200.0;

/// What each Gmail call costs in quota units, as Google lists them.
mod units {
    pub const LABEL_GET: u32 = 1;
    pub const LABELS_LIST: u32 = 1;
    pub const LABEL_CREATE: u32 = 5;
    pub const LABEL_DELETE: u32 = 5;
    pub const PROFILE_GET: u32 = 1;
    pub const MESSAGES_LIST: u32 = 5;
    pub const HISTORY_LIST: u32 = 2;
    pub const MESSAGE_GET: u32 = 5;
    pub const MESSAGE_IMPORT: u32 = 25;
    pub const MESSAGE_DELETE: u32 = 10;
    pub const MESSAGE_MODIFY: u32 = 5;
    pub const BATCH_MODIFY: u32 = 50;
}

/// The live Gmail session of one side.
pub struct GmailClient {
    inner: GmailClientStd,
    /// The TLS configuration, kept for stream reopens.
    tls: Tls,
    /// Label name to label id, refreshed from the label listing when a name
    /// misses.
    labels: HashMap<String, String>,
    /// Whether the server allowed reusing the stream after the last
    /// exchange; when false the next operation reopens it.
    alive: bool,
    /// The source's pacing and back-off, shared by its connections.
    throttle: Arc<Throttle>,
    /// The flag sets of the last label and query a round page read, kept
    /// for the run so the next page lists them again for nothing.
    flag_sets: Option<((String, Option<String>), FlagSets)>,
}

impl GmailClient {
    /// Opens the TLS connection to the Gmail API with a bearer token, scoped
    /// to the `user` mailbox owner (`me` or an address).
    pub fn connect(
        token: &SecretString,
        user: &str,
        tls: Tls,
        throttle: Arc<Throttle>,
    ) -> Result<Self> {
        let options = GmailClientStdConnectOptions {
            tls: tls.clone(),
            proxy: Proxy::None,
            user_id: user.to_owned(),
        };
        let inner = GmailClientStd::connect(token.expose_secret(), options)
            .context("Cannot connect to Gmail")?;

        Ok(Self {
            inner,
            tls,
            labels: HashMap::new(),
            alive: true,
            throttle,
            flag_sets: None,
        })
    }

    /// Reopens the stream to the Gmail endpoint, keeping the credential.
    fn reconnect(&mut self) -> Result<()> {
        debug!("reopening the gmail stream");

        let url = Url::parse(GMAIL_API_BASE).context("Cannot parse the Gmail API base URL")?;
        let host = url.host_str().context("Gmail API base URL has no host")?;
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

    /// Runs one Gmail operation costing `units` of quota, reopening the
    /// stream first when the server closed it, and records the new
    /// keep-alive hint.
    ///
    /// The source's [`Throttle`] paces it and sends it again while Gmail
    /// throttles it (429, 503, or a 403 rate limit). A request creating
    /// something goes through [`create`](Self::create) instead.
    fn op<T>(
        &mut self,
        units: u32,
        run: impl FnMut(&mut GmailClientStd) -> Result<GmailSendOutput<T>, GmailClientStdError>,
    ) -> Result<T, GmailClientStdError> {
        self.send(Request::Idempotent, units, run)
    }

    /// Runs one Gmail request that creates something (a label, an imported
    /// message): sent again on a refusal Gmail answers before doing anything
    /// (429, a rate limit), never on a 503, after which it may exist.
    fn create<T>(
        &mut self,
        units: u32,
        run: impl FnMut(&mut GmailClientStd) -> Result<GmailSendOutput<T>, GmailClientStdError>,
    ) -> Result<T, GmailClientStdError> {
        self.send(Request::Create, units, run)
    }

    /// Runs one Gmail request of `request`, as [`op`](Self::op) describes.
    fn send<T>(
        &mut self,
        request: Request,
        units: u32,
        mut run: impl FnMut(&mut GmailClientStd) -> Result<GmailSendOutput<T>, GmailClientStdError>,
    ) -> Result<T, GmailClientStdError> {
        let throttle = Arc::clone(&self.throttle);
        throttle.call(
            units,
            || {
                if !self.alive
                    && let Err(err) = self.reconnect()
                {
                    warn!("cannot reopen the gmail stream: {err:#}");
                }

                let out = run(&mut self.inner)?;
                self.alive = out.keep_alive;
                Ok(out.response)
            },
            |err| match err {
                GmailClientStdError::Send(GmailSendError::Api { status, message }) => {
                    google_throttled_for(request, *status, message)
                }
                _ => None,
            },
            |message| {
                GmailClientStdError::Send(GmailSendError::Api {
                    status: 429,
                    message,
                })
            },
        )
    }

    /// Lists the user labels and [`SYSTEM_COLLECTIONS`], counted when
    /// `with_counts` at one label get each.
    ///
    /// The name map is refreshed as a side effect.
    pub fn list_collections(&mut self, with_counts: bool) -> Result<Vec<Collection>> {
        let labels = self.list_labels()?;

        let mut collections = Vec::with_capacity(labels.len());
        for label in labels {
            let (total, unread) = if with_counts {
                let label = self
                    .op(units::LABEL_GET, |client| client.label_get(&label.id))
                    .with_context(|| format!("Get label {} error", label.name))?;
                (label.messages_total, label.messages_unread)
            } else {
                (None, None)
            };

            let role = system_role(&label.id).map(String::from);
            collections.push(Collection {
                id: label.name.clone(),
                name: label.name,
                total,
                unread,
                role,
            });
        }

        Ok(collections)
    }

    /// The role each system label states, by collection name. A check-only
    /// probe; user labels state none.
    pub fn mailbox_roles(&mut self) -> Result<BTreeMap<String, String>> {
        Ok(self
            .list_labels()?
            .into_iter()
            .filter_map(|label| Some((label.name, system_role(&label.id)?.to_owned())))
            .collect())
    }

    /// Lists the labels synced as collections, refreshing the name map.
    fn list_labels(&mut self) -> Result<Vec<GmailLabel>> {
        let labels: Vec<GmailLabel> = self
            .op(units::LABELS_LIST, |client| client.labels_list())
            .context("List labels error")?
            .labels
            .into_iter()
            .filter(is_collection)
            .collect();

        self.labels = labels
            .iter()
            .map(|label| (label.name.clone(), label.id.clone()))
            .collect();

        trace!("labels: {:?}", self.labels.keys());
        Ok(labels)
    }

    /// Resolves a collection name to its label id, case-insensitively,
    /// refreshing the name map on a miss.
    fn label_id(&mut self, name: &str) -> Result<String> {
        if let Some(id) = lookup_label(&self.labels, name) {
            return Ok(id);
        }
        self.list_labels()?;
        lookup_label(&self.labels, name).with_context(|| format!("Unknown Gmail label {name}"))
    }

    /// Creates the label `name` under `parent`, the top level when `None`,
    /// unless a label already goes by that name, and answers the collection
    /// name it is listed under (pimdir Annex B.2 `collection-create`).
    pub fn create_child_label(&mut self, parent: Option<&str>, name: &str) -> Result<String> {
        let label = child_label(parent, name)?;
        self.list_labels()?;
        if let Some((existing, _)) = self
            .labels
            .iter()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(&label.name))
        {
            return Ok(existing.clone());
        }

        let created = self
            .create(units::LABEL_CREATE, |client| client.label_create(&label))
            .with_context(|| format!("Create label {} error", label.name))?;
        let name = created.name.clone();
        self.labels.insert(created.name, created.id);
        Ok(name)
    }

    /// Creates a user label; a system label is never created.
    pub fn create_collection(&mut self, name: &str) -> Result<()> {
        if is_system(name) {
            bail!("Gmail system label {name} cannot be created");
        }

        let label = GmailLabel {
            name: name.to_owned(),
            ..Default::default()
        };
        let label = self
            .create(units::LABEL_CREATE, |client| client.label_create(&label))
            .with_context(|| format!("Create label {name} error"))?;

        self.labels.insert(label.name, label.id);
        Ok(())
    }

    /// Deletes a user label, its messages keeping their other labels; a
    /// system label is never deleted.
    pub fn delete_collection(&mut self, name: &str) -> Result<()> {
        if is_system(name) {
            bail!("Gmail system label {name} cannot be deleted");
        }

        let id = self.label_id(name)?;
        self.op(units::LABEL_DELETE, |client| client.label_delete(&id))
            .with_context(|| format!("Delete label {name} error"))?;

        self.labels.retain(|_, label| *label != id);
        Ok(())
    }

    /// Lists one page of a label (pimdir SYNC §4).
    ///
    /// A delta reads the label's history since the checkpoint's history id,
    /// an expired one (HTTP 404) being a rejected checkpoint. A round lists
    /// the label newest first, [`ROUND_PAGE_SIZE`] ids a page under the
    /// scope's `after:` (two days below `since`, a superset of the scope on
    /// `Date`), its cursor the page token; its first page carries the
    /// profile's history id, read before the listing, so what lands during
    /// the round is the next delta's.
    pub fn enumerate(
        &mut self,
        collection: &str,
        request: &PimdirEnumerate,
        held: Held<'_>,
    ) -> Result<Listed> {
        let label = self.label_id(collection)?;

        match &request.listing {
            PimdirListing::Delta(checkpoint) => {
                let Some(start) = decode_checkpoint(&checkpoint.0) else {
                    return Ok(Listed::CursorRejected);
                };
                match self.history(&label, &start, DELTA_HISTORY) {
                    Ok((records, next)) => self
                        .delta_round(collection, &label, &records, next, held)
                        .map(Listed::Page),
                    Err(err) if is_not_found(&err) => {
                        warn!("gmail history of {collection} expired, restarting a round");
                        Ok(Listed::CursorRejected)
                    }
                    Err(err) => Err(anyhow::Error::new(err)
                        .context(format!("List history of {collection} error"))),
                }
            }
            PimdirListing::Round { cursor, band } => {
                let token = match cursor {
                    None => None,
                    Some(cursor) => match decode_checkpoint(&cursor.0) {
                        Some(token) => Some(token),
                        None => return Ok(Listed::CursorRejected),
                    },
                };
                self.round_page(collection, &label, token, *band, &request.scope, held)
            }
        }
    }

    /// One page of a round over `scope`, from its start or from `token`.
    fn round_page(
        &mut self,
        collection: &str,
        label: &str,
        token: Option<String>,
        band: bool,
        scope: &PimdirScope,
        held: Held<'_>,
    ) -> Result<Listed> {
        debug!("begin gmail round page");
        trace!("collection: {collection}, resumed: {}", token.is_some());

        let checkpoint = match token.is_none() && !band {
            true => Some(
                self.op(units::PROFILE_GET, |client| client.profile_get())
                    .context("Get Gmail profile error")?
                    .history_id
                    .context("Gmail profile carries no history id")?
                    .into_bytes(),
            ),
            false => None,
        };

        let query = scope_query(scope);
        let label_ids = [label.to_owned()];
        let params = GmailMessagesListParams {
            q: query.as_deref(),
            label_ids: &label_ids,
            max_results: Some(ROUND_PAGE_SIZE),
            page_token: token.as_deref(),
            include_spam_trash: label == SPAM || label == TRASH,
        };
        let page = match self.op(units::MESSAGES_LIST, |client| client.messages_list(&params)) {
            Ok(page) => page,
            Err(err) if token.is_some() && is_bad_request(&err) => {
                warn!("gmail page token of {collection} refused, restarting the round");
                return Ok(Listed::CursorRejected);
            }
            Err(err) => {
                return Err(
                    anyhow::Error::new(err).context(format!("List messages of {collection} error"))
                );
            }
        };

        let ids: Vec<String> = page
            .messages
            .into_iter()
            .map(|message| message.id)
            .collect();
        let (bound, unbound): (Vec<String>, Vec<String>) =
            ids.into_iter().partition(|id| held.holds(id));

        let mut items = Vec::with_capacity(bound.len() + unbound.len());
        if !bound.is_empty() {
            let flags = self.flag_sets(label, query.as_deref())?;
            items.extend(bound.into_iter().map(|id| {
                let flags = flags.of(&id);
                EnumEntry::bare(id, flags, None)
            }));
        }

        let mut bytes = 0;
        for chunk in unbound.chunks(META_CHUNK) {
            for id in chunk {
                match self.metadata(id) {
                    Ok(message) => {
                        bytes += serde_json::to_vec(&message).map_or(0, |json| json.len() as u64);
                        items.push(named_entry(id, &message));
                    }
                    // NOTE: gone since the listing: absent from the round,
                    // as it is from the label.
                    Err(err) if is_not_found(&err) => continue,
                    Err(err) => {
                        return Err(anyhow::Error::new(err)
                            .context(format!("Get message {id} metadata error")));
                    }
                }
            }
        }

        debug!("end of gmail round page");
        trace!(
            "items: {}, last: {}",
            items.len(),
            page.next_page_token.is_none()
        );

        Ok(Listed::Page(Enumeration {
            items,
            vanished: Vec::new(),
            complete: true,
            cursor: page.next_page_token.map(String::into_bytes),
            checkpoint,
            bytes,
        }))
    }

    /// The flag-like labels of a label's messages under `query`, as three
    /// id-only listings read once a run rather than one get a message.
    fn flag_sets(&mut self, label: &str, query: Option<&str>) -> Result<FlagSets> {
        let key = (label.to_owned(), query.map(String::from));
        if let Some((cached, sets)) = &self.flag_sets
            && *cached == key
        {
            return Ok(sets.clone());
        }

        let sets = FlagSets {
            unread: self
                .list_ids(label, Some(UNREAD), query)?
                .into_iter()
                .collect(),
            starred: self
                .list_ids(label, Some(STARRED), query)?
                .into_iter()
                .collect(),
            important: self
                .list_ids(label, Some(IMPORTANT), query)?
                .into_iter()
                .collect(),
        };
        self.flag_sets = Some((key, sets.clone()));
        Ok(sets)
    }

    /// One message's metadata: its labels and the headers that name it.
    fn metadata(&mut self, id: &str) -> Result<GmailMessage, GmailClientStdError> {
        self.op(units::MESSAGE_GET, |client| {
            client.message_get(id, GmailMessageFormat::Metadata, SUMMARY_HEADERS)
        })
    }

    /// Lists the ids of a label's messages, narrowed to the ones also
    /// carrying `and` when given and to `query`.
    ///
    /// Spam and trash are listed only from their own label, as Gmail's own
    /// views do.
    fn list_ids(
        &mut self,
        label: &str,
        and: Option<&str>,
        query: Option<&str>,
    ) -> Result<Vec<String>> {
        let label_ids: Vec<String> = [Some(label), and]
            .into_iter()
            .flatten()
            .map(String::from)
            .collect();

        let mut ids = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let params = GmailMessagesListParams {
                q: query,
                label_ids: &label_ids,
                max_results: Some(PAGE_SIZE),
                page_token: token.as_deref(),
                include_spam_trash: label == SPAM || label == TRASH,
            };
            let page = self
                .op(units::MESSAGES_LIST, |client| client.messages_list(&params))
                .with_context(|| format!("List messages of {label_ids:?} error"))?;

            ids.extend(page.messages.into_iter().map(|message| message.id));
            match page.next_page_token {
                Some(next) => token = Some(next),
                None => return Ok(ids),
            }
        }
    }

    /// Reads a label's history since `start`, paged, returning the records
    /// and the mailbox history id the next round starts from.
    ///
    /// The error comes back unwrapped, so the caller can tell an expired
    /// history id (404) from any other failure.
    fn history(
        &mut self,
        label: &str,
        start: &str,
        types: &[GmailHistoryType],
    ) -> Result<(Vec<GmailHistory>, String), GmailClientStdError> {
        let mut records = Vec::new();
        let mut checkpoint = start.to_owned();
        let mut token: Option<String> = None;
        loop {
            let params = GmailHistoryListParams {
                start_history_id: start,
                label_id: Some(label),
                history_types: types,
                max_results: Some(PAGE_SIZE),
                page_token: token.as_deref(),
            };
            let page = self.op(units::HISTORY_LIST, |client| {
                let coroutine = GmailHistoryList::new(&client.auth, &client.user_id, &params)?;
                client.run(coroutine)
            })?;

            records.extend(page.history);
            if let Some(id) = page.history_id {
                checkpoint = id;
            }
            match page.next_page_token {
                Some(next) => token = Some(next),
                None => return Ok((records, checkpoint)),
            }
        }
    }

    /// Folds a label's history into a delta.
    ///
    /// History records are increments, not state, so every changed message's
    /// current labels are read before it is reported: a minimal get for one
    /// the store binds, its metadata for one it does not, which names it.
    /// Still carrying the label, it is an item with its flags; otherwise or
    /// gone, it vanished.
    fn delta_round(
        &mut self,
        collection: &str,
        label: &str,
        records: &[GmailHistory],
        checkpoint: String,
        held: Held<'_>,
    ) -> Result<Enumeration> {
        debug!("begin gmail delta round");
        trace!("collection: {collection}, records: {}", records.len());

        let (changed, deleted) = history_changes(records);

        let mut items = Vec::new();
        let mut vanished: Vec<String> = deleted.into_iter().collect();
        let mut bytes = 0;
        for id in changed {
            let bound = held.holds(&id);
            let got = match bound {
                true => self.op(units::MESSAGE_GET, |client| {
                    client.message_get(&id, GmailMessageFormat::Minimal, &[])
                }),
                false => self.metadata(&id),
            };
            match got {
                Ok(message) if belongs(&message.label_ids, label) => match bound {
                    true => items.push(EnumEntry::bare(
                        id,
                        flags_from_labels(&message.label_ids),
                        None,
                    )),
                    false => {
                        bytes += serde_json::to_vec(&message).map_or(0, |json| json.len() as u64);
                        items.push(named_entry(&id, &message));
                    }
                },
                Ok(_) => vanished.push(id),
                Err(err) if is_not_found(&err) => vanished.push(id),
                Err(err) => {
                    return Err(anyhow::Error::new(err).context(format!("Get message {id} error")));
                }
            }
        }

        debug!("end of gmail delta round");
        trace!("items: {}, vanished: {}", items.len(), vanished.len());

        let mut delta = Enumeration::delta(items, vanished, checkpoint.into_bytes());
        delta.bytes = bytes;
        Ok(delta)
    }

    /// Fetches the summaries of an id set, one metadata get per message.
    pub fn fetch_summaries(&mut self, ids: &[&str]) -> Result<Vec<ItemSummary>> {
        let mut summaries = Vec::with_capacity(ids.len());
        for id in ids {
            let message = self
                .op(units::MESSAGE_GET, |client| {
                    client.message_get(id, GmailMessageFormat::Metadata, SUMMARY_HEADERS)
                })
                .with_context(|| format!("Get message {id} metadata error"))?;
            summaries.push(message_summary(id, &message));
        }
        Ok(summaries)
    }

    /// Streams the bodies of an id set, one raw get per message, Gmail
    /// having no batched body fetch.
    pub fn fetch_bodies<S: Write>(
        &mut self,
        ids: &[&str],
        mut open: impl FnMut(&str) -> std::io::Result<S>,
        mut done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<()> {
        for id in ids {
            let raw = self.message_raw(id)?;
            let mut sink = open(id).with_context(|| format!("Open body sink for {id} error"))?;
            sink.write_all(&raw)
                .with_context(|| format!("Store body {id} error"))?;
            done(id, None, sink).with_context(|| format!("Commit body {id} error"))?;
        }
        Ok(())
    }

    /// Streams one message's raw RFC 5322 bytes into `sink`.
    pub fn get_item_stream(&mut self, id: &str, mut sink: impl Write) -> Result<Option<String>> {
        let raw = self.message_raw(id)?;
        sink.write_all(&raw)
            .with_context(|| format!("Stream body {id} error"))?;
        Ok(None)
    }

    /// Fetches the raw RFC 5322 bytes of one message, base64url-decoded.
    fn message_raw(&mut self, id: &str) -> Result<Vec<u8>> {
        let message = self
            .op(units::MESSAGE_GET, |client| {
                client.message_get(id, GmailMessageFormat::Raw, &[])
            })
            .with_context(|| format!("Get raw message {id} error"))?;
        let raw = message
            .raw
            .with_context(|| format!("Raw message {id} carries no body"))?;
        decode_raw(&raw).map_err(|err| anyhow!("Decode raw message {id} error: {err}"))
    }

    /// Imports a message into a label with its flags, keeping its own date
    /// and `Message-ID`, and returns the id it really got.
    ///
    /// `import` may answer an id that 404s, so the answer is checked, and
    /// on a miss the message is found again by `hint` (its `Message-ID`)
    /// among the ones the label's history added since the call.
    pub fn add_item_stream(
        &mut self,
        collection: &str,
        flags: &[Flag],
        mut source: impl Read,
        hint: Option<&str>,
    ) -> Result<WrittenItem> {
        let label = self.label_id(collection)?;

        let mut raw = Vec::new();
        source
            .read_to_end(&mut raw)
            .context("Read message to import error")?;

        let start = self
            .op(units::PROFILE_GET, |client| client.profile_get())
            .context("Get Gmail profile error")?
            .history_id
            .context("Gmail profile carries no history id")?;

        let (mut label_ids, _) = label_patch(flags, FlagOp::Set);
        label_ids.insert(0, label.clone());
        let message = GmailMessage {
            raw: Some(encode_raw(&raw)),
            label_ids,
            ..Default::default()
        };
        let answered = self
            .create(units::MESSAGE_IMPORT, |client| {
                let coroutine = GmailMessageImport::new(
                    &client.auth,
                    &client.user_id,
                    &message,
                    Some(GmailInternalDateSource::DateHeader),
                    false,
                    true,
                    false,
                )?;
                client.run(coroutine)
            })
            .with_context(|| format!("Import message into {collection} error"))?
            .id;

        match self.op(units::MESSAGE_GET, |client| {
            client.message_get(&answered, GmailMessageFormat::Minimal, &[])
        }) {
            Ok(_) => {
                return Ok(WrittenItem {
                    id: answered,
                    revision: None,
                });
            }
            Err(err) if is_not_found(&err) => {
                debug!("imported id {answered} not found, searching the history");
            }
            Err(err) => {
                return Err(
                    anyhow::Error::new(err).context(format!("Get message {answered} error"))
                );
            }
        }

        let Some(hint) = hint.and_then(normalize_message_id) else {
            bail!(
                "Gmail answered import id {answered}, which does not exist, and the message has no Message-ID to find it by"
            );
        };

        for attempt in 1..=IMPORT_ATTEMPTS {
            let (records, _) = self
                .history(&label, &start, &[GmailHistoryType::MessageAdded])
                .with_context(|| format!("List history of {collection} error"))?;

            for id in history_added(&records) {
                let message = self
                    .op(units::MESSAGE_GET, |client| {
                        client.message_get(&id, GmailMessageFormat::Metadata, &["Message-ID"])
                    })
                    .with_context(|| format!("Get message {id} metadata error"))?;
                let message_id = message
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.header("Message-ID"))
                    .and_then(normalize_message_id);
                if message_id.as_deref() == Some(hint.as_str()) {
                    return Ok(WrittenItem { id, revision: None });
                }
            }

            if attempt < IMPORT_ATTEMPTS {
                thread::sleep(Duration::from_secs(1));
            }
        }

        bail!(
            "Gmail answered import id {answered}, which does not exist, and the history of {collection} shows no message <{hint}>"
        )
    }

    /// Deletes one message from a label by removing that label, which
    /// archives when it is `INBOX`; from `TRASH` it is deleted for good.
    pub fn delete_item(&mut self, collection: &str, id: &str) -> Result<()> {
        let label = self.label_id(collection)?;

        if label == TRASH {
            self.op(units::MESSAGE_DELETE, |client| client.message_delete(id))
                .with_context(|| format!("Delete message {id} error"))?;
        } else {
            self.modify(&[id], &[], &[label])
                .with_context(|| format!("Remove {collection} from message {id} error"))?;
        }

        Ok(())
    }

    /// Moves messages from one label to another in one modify each batch.
    pub fn move_items(&mut self, from: &str, to: &str, ids: &[&str]) -> Result<()> {
        let from_label = self.label_id(from)?;
        let to_label = self.label_id(to)?;
        if from_label == to_label {
            return Ok(());
        }

        self.modify(ids, &[to_label], &[from_label])
            .with_context(|| format!("Move messages from {from} to {to} error"))
    }

    /// Adds, sets or removes flags as `UNREAD`, `STARRED` and `IMPORTANT`
    /// label changes; flags with no Gmail label are dropped.
    pub fn store_flags(&mut self, ids: &[&str], flags: &[Flag], op: FlagOp) -> Result<()> {
        let (add, remove) = label_patch(flags, op);
        if add.is_empty() && remove.is_empty() {
            return Ok(());
        }

        self.modify(ids, &add, &remove)
            .context("Update message flags error")
    }

    /// Adds and removes labels on an id set: one `modify` for one message,
    /// `batchModify` by chunks otherwise.
    fn modify(&mut self, ids: &[&str], add: &[String], remove: &[String]) -> Result<()> {
        if let [id] = ids {
            self.op(units::MESSAGE_MODIFY, |client| {
                client.message_modify(id, add, remove)
            })?;
            return Ok(());
        }

        for chunk in ids.chunks(BATCH_SIZE) {
            let chunk: Vec<String> = chunk.iter().map(|id| id.to_string()).collect();
            self.op(units::BATCH_MODIFY, |client| {
                let coroutine = GmailMessagesBatchModify::new(
                    &client.auth,
                    &client.user_id,
                    &chunk,
                    add,
                    remove,
                )?;
                client.run(coroutine)
            })?;
        }

        Ok(())
    }
}

/// Whether a client error is a missing resource (HTTP 404): an expired
/// history id, or a message that does not exist.
fn is_not_found(err: &GmailClientStdError) -> bool {
    matches!(err, GmailClientStdError::Send(send) if send.status() == Some(404))
}

/// Whether Gmail refused a request as malformed (HTTP 400), which is how it
/// answers a page token it no longer takes.
fn is_bad_request(err: &GmailClientStdError) -> bool {
    matches!(err, GmailClientStdError::Send(send) if send.status() == Some(400))
}

/// The flag-like labels of a label's messages, as id sets.
#[derive(Clone, Debug, Default)]
struct FlagSets {
    unread: BTreeSet<String>,
    starred: BTreeSet<String>,
    important: BTreeSet<String>,
}

impl FlagSets {
    /// The shared flag set of message `id`.
    fn of(&self, id: &str) -> BTreeSet<Flag> {
        flags(
            self.unread.contains(id),
            self.starred.contains(id),
            self.important.contains(id),
        )
    }
}

/// A listed message named by its metadata.
fn named_entry(id: &str, message: &GmailMessage) -> EnumEntry {
    EnumEntry {
        id: id.to_owned(),
        flags: flags_from_labels(&message.label_ids),
        revision: None,
        meta: Some(mail::parse_summary(&message_summary(id, message))),
    }
}

/// The search narrowing a listing to a scope: `after:` [`SCOPE_MARGIN_DAYS`]
/// below `since` and `before:` as many above `until`, in epoch seconds,
/// which Gmail reads as instants rather than its own midnights. A superset
/// of the messages whose `Date` is in scope, which the local check decides.
fn scope_query(scope: &PimdirScope) -> Option<String> {
    let epoch = |instant: &str, shift: i64| {
        let at = DateTime::parse_from_rfc3339(instant).ok()?.to_utc();
        let at = at.checked_add_signed(chrono::Duration::days(shift))?;
        Some(at.timestamp())
    };
    let terms: Vec<String> = [
        scope
            .since
            .as_deref()
            .and_then(|since| epoch(since, -SCOPE_MARGIN_DAYS))
            .map(|at| format!("after:{at}")),
        scope
            .until
            .as_deref()
            .and_then(|until| epoch(until, SCOPE_MARGIN_DAYS))
            .map(|at| format!("before:{at}")),
    ]
    .into_iter()
    .flatten()
    .collect();

    (!terms.is_empty()).then(|| terms.join(" "))
}

/// Whether a label is synced as a collection: every user label, and the
/// [`SYSTEM_COLLECTIONS`].
fn is_collection(label: &GmailLabel) -> bool {
    match label.label_type {
        Some(GmailLabelType::User) => !label.id.is_empty() && !label.name.is_empty(),
        _ => is_system(&label.id),
    }
}

/// The role a system label states, by its id.
fn system_role(id: &str) -> Option<&'static str> {
    match id {
        "INBOX" => Some("inbox"),
        "SENT" => Some("sent"),
        "DRAFT" => Some("drafts"),
        SPAM => Some("junk"),
        TRASH => Some("trash"),
        _ => None,
    }
}

/// The label a `collection-create` asks Gmail for: `name` under `parent`,
/// Gmail nesting a label by its name (`Parent/Child`). A system label
/// takes no child, and a name holding `/` would nest by itself.
fn child_label(parent: Option<&str>, name: &str) -> Result<GmailLabel> {
    if name.trim().is_empty() || name.contains('/') {
        return Err(anyhow::Error::new(Refusal(format!(
            "A Gmail label name is not blank and holds no /: {name}"
        ))));
    }
    let name = match parent {
        Some(parent) if is_system(parent) => {
            return Err(anyhow::Error::new(Refusal(format!(
                "Gmail system label {parent} takes no child label"
            ))));
        }
        Some(parent) => format!("{parent}/{name}"),
        None if is_system(name) => {
            return Err(anyhow::Error::new(Refusal(format!(
                "Gmail system label {name} cannot be created"
            ))));
        }
        None => name.to_owned(),
    };

    Ok(GmailLabel {
        name,
        ..Default::default()
    })
}

/// Whether a collection name is one of the [`SYSTEM_COLLECTIONS`], whose
/// name is their id.
fn is_system(name: &str) -> bool {
    SYSTEM_COLLECTIONS.contains(&name)
}

/// Finds a label id by collection name, case-insensitively, as Gmail
/// refuses two labels differing by case.
fn lookup_label(labels: &HashMap<String, String>, name: &str) -> Option<String> {
    labels
        .iter()
        .find(|(label, _)| label.eq_ignore_ascii_case(name))
        .map(|(_, id)| id.clone())
}

/// Whether a message carrying `label_ids` is a member of `label`'s
/// collection: a spam or trashed message is one of `SPAM` or `TRASH` only,
/// as the listings exclude it everywhere else.
fn belongs(label_ids: &[String], label: &str) -> bool {
    let has = |id: &str| label_ids.iter().any(|l| l == id);
    has(label) && (label == SPAM || label == TRASH || !(has(SPAM) || has(TRASH)))
}

/// The ids a label's history touched, in first-seen order, against the ones
/// it deleted outright.
fn history_changes(records: &[GmailHistory]) -> (Vec<String>, BTreeSet<String>) {
    let deleted: BTreeSet<String> = records
        .iter()
        .flat_map(|record| &record.messages_deleted)
        .map(|deleted| deleted.message.id.clone())
        .filter(|id| !id.is_empty())
        .collect();

    let mut seen = BTreeSet::new();
    let changed = records
        .iter()
        .flat_map(|record| {
            let added = record.messages_added.iter().map(|added| &added.message);
            let labelled = record.labels_added.iter().map(|change| &change.message);
            let unlabelled = record.labels_removed.iter().map(|change| &change.message);
            added.chain(labelled).chain(unlabelled)
        })
        .map(|message| message.id.clone())
        .filter(|id| !id.is_empty() && !deleted.contains(id) && seen.insert(id.clone()))
        .collect();

    (changed, deleted)
}

/// The ids a history reports added, in order, without duplicates.
fn history_added(records: &[GmailHistory]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    records
        .iter()
        .flat_map(|record| &record.messages_added)
        .map(|added| added.message.id.clone())
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

/// The shared flag set of a message from its flag-like labels.
fn flags(unread: bool, starred: bool, important: bool) -> BTreeSet<Flag> {
    let mut flags = BTreeSet::new();
    if !unread {
        flags.insert(Flag::from_iana(IanaFlag::Seen));
    }
    if starred {
        flags.insert(Flag::from_iana(IanaFlag::Flagged));
    }
    if important {
        flags.insert(Flag::from_iana(IanaFlag::Important));
    }
    flags
}

/// Derives the shared flag set from a message's label ids.
fn flags_from_labels(label_ids: &[String]) -> BTreeSet<Flag> {
    let has = |label: &str| label_ids.iter().any(|id| id == label);
    flags(has(UNREAD), has(STARRED), has(IMPORTANT))
}

/// Translates a flag-store operation into Gmail `(addLabelIds,
/// removeLabelIds)`.
///
/// `\Seen` maps to the absence of `UNREAD`, so its polarity is inverted.
/// Every other flag is dropped: drafts and spam are collections here, and a
/// custom keyword turned label would file the message in a new collection.
fn label_patch(flags: &[Flag], op: FlagOp) -> (Vec<String>, Vec<String>) {
    let mut add = Vec::new();
    let mut remove = Vec::new();

    match op {
        FlagOp::Add | FlagOp::Remove => {
            let adding = matches!(op, FlagOp::Add);
            for (label, inverted) in flags.iter().filter_map(label_of) {
                if adding ^ inverted {
                    &mut add
                } else {
                    &mut remove
                }
                .push(label.to_owned());
            }
        }
        FlagOp::Set => {
            let has = |iana: IanaFlag| flags.iter().any(|flag| flag.iana() == Some(iana));
            for (wanted, label, inverted) in [
                (has(IanaFlag::Seen), UNREAD, true),
                (has(IanaFlag::Flagged), STARRED, false),
                (has(IanaFlag::Important), IMPORTANT, false),
            ] {
                if wanted ^ inverted {
                    &mut add
                } else {
                    &mut remove
                }
                .push(label.to_owned());
            }
        }
    }

    (add, remove)
}

/// Maps a shared flag to its Gmail `(label, inverted)` pair, or `None` when
/// the flag has no Gmail label.
fn label_of(flag: &Flag) -> Option<(&'static str, bool)> {
    match flag.iana() {
        Some(IanaFlag::Seen) => Some((UNREAD, true)),
        Some(IanaFlag::Flagged) => Some((STARRED, false)),
        Some(IanaFlag::Important) => Some((IMPORTANT, false)),
        _ => None,
    }
}

/// Folds a metadata get into a shared [`ItemSummary`] for the `Meta` tier.
///
/// The headers go through io-pimdir's own decoders, the ones the `Full`
/// derivation reads a body with, so both tiers agree byte-for-byte. The
/// size is Gmail's estimate, restated from the body at the `Full` tier.
fn message_summary(id: &str, message: &GmailMessage) -> ItemSummary {
    let header = |name: &str| {
        message
            .payload
            .as_ref()
            .and_then(|payload| payload.header(name))
    };
    let list = |name: &str| {
        header(name)
            .map(addresses)
            .unwrap_or_default()
            .into_iter()
            .map(|address| Address {
                name: address.name,
                email: address.address,
            })
            .collect()
    };

    ItemSummary {
        id: id.to_owned(),
        message_id: header("Message-ID").and_then(normalize_message_id),
        in_reply_to: header("In-Reply-To").map(message_ids).unwrap_or_default(),
        flags: flags_from_labels(&message.label_ids),
        subject: header("Subject").map(decode).unwrap_or_default(),
        from: list("From"),
        to: list("To"),
        cc: list("Cc"),
        bcc: list("Bcc"),
        date: header("Date")
            .and_then(instant)
            .and_then(|date| DateTime::parse_from_rfc3339(&date).ok()),
        size: message.size_estimate.unwrap_or(0),
        // NOTE: read without the body (pimdir STORAGE Annex A.1), which the
        // walk of the parts replaces once the body is in.
        has_attachment: Some(meta_attachment(header("Content-Type"))),
    }
}

/// Decodes checkpoint bytes back into a history id; `None` for an absent,
/// empty or non-UTF-8 checkpoint, which forces a full round.
fn decode_checkpoint(bytes: &[u8]) -> Option<String> {
    let id = std::str::from_utf8(bytes).ok()?;
    (!id.is_empty()).then(|| id.to_owned())
}

#[cfg(test)]
mod tests {
    use io_gmail::v1::send::GmailSendError;

    use super::*;

    #[test]
    fn a_created_label_nests_under_its_parent_by_name() {
        assert_eq!(child_label(None, "Archives").unwrap().name, "Archives");
        assert_eq!(
            child_label(Some("Projects"), "2026").unwrap().name,
            "Projects/2026"
        );

        let refused = |parent: Option<&str>, name: &str| {
            child_label(parent, name)
                .unwrap_err()
                .chain()
                .any(|cause| cause.is::<Refusal>())
        };
        assert!(refused(Some("INBOX"), "Sub"));
        assert!(refused(None, "TRASH"));
        assert!(refused(None, "a/b"));
        assert!(refused(None, " "));
    }

    #[test]
    fn system_labels_state_their_role_user_labels_none() {
        assert_eq!(system_role("INBOX"), Some("inbox"));
        assert_eq!(system_role("DRAFT"), Some("drafts"));
        assert_eq!(system_role("SPAM"), Some("junk"));
        assert_eq!(system_role("Label_12"), None);
    }

    fn labels(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn user_labels_and_five_system_labels_are_collections() {
        let listed: Vec<GmailLabel> = serde_json::from_str(
            r#"[
                {"id": "INBOX", "name": "INBOX", "type": "system"},
                {"id": "TRASH", "name": "TRASH", "type": "system"},
                {"id": "UNREAD", "name": "UNREAD", "type": "system"},
                {"id": "STARRED", "name": "STARRED", "type": "system"},
                {"id": "CATEGORY_SOCIAL", "name": "CATEGORY_SOCIAL", "type": "system"},
                {"id": "CHAT", "name": "CHAT", "type": "system"},
                {"id": "Label_1", "name": "Parent/Child", "type": "user"}
            ]"#,
        )
        .unwrap();

        let kept: Vec<&str> = listed
            .iter()
            .filter(|label| is_collection(label))
            .map(|label| label.name.as_str())
            .collect();
        assert_eq!(kept, ["INBOX", "TRASH", "Parent/Child"]);

        let map = HashMap::from([(String::from("Parent/Child"), String::from("Label_1"))]);
        assert_eq!(
            lookup_label(&map, "parent/child").as_deref(),
            Some("Label_1")
        );
        assert_eq!(lookup_label(&map, "Missing"), None);
    }

    #[test]
    fn flag_labels_map_to_iana_flags() {
        assert_eq!(
            flags_from_labels(&labels(&["INBOX", "STARRED", "IMPORTANT"])),
            BTreeSet::from([
                Flag::from_iana(IanaFlag::Seen),
                Flag::from_iana(IanaFlag::Flagged),
                Flag::from_iana(IanaFlag::Important),
            ])
        );
        assert!(flags_from_labels(&labels(&["INBOX", "UNREAD", "DRAFT"])).is_empty());
    }

    #[test]
    fn a_label_patch_inverts_seen_and_drops_the_rest() {
        let seen = Flag::from_iana(IanaFlag::Seen);
        let flagged = Flag::from_iana(IanaFlag::Flagged);
        let draft = Flag::from_iana(IanaFlag::Draft);
        let custom = Flag::from_raw("$Work");

        assert_eq!(
            label_patch(&[seen.clone(), draft.clone(), custom.clone()], FlagOp::Add),
            (vec![], labels(&["UNREAD"]))
        );
        assert_eq!(
            label_patch(&[seen.clone(), flagged.clone()], FlagOp::Remove),
            (labels(&["UNREAD"]), labels(&["STARRED"]))
        );
        assert_eq!(
            label_patch(&[flagged, draft, custom], FlagOp::Set),
            (labels(&["UNREAD", "STARRED"]), labels(&["IMPORTANT"]))
        );
    }

    #[test]
    fn spam_and_trash_belong_to_their_own_label_only() {
        assert!(belongs(&labels(&["INBOX", "UNREAD"]), "INBOX"));
        assert!(!belongs(&labels(&["SENT"]), "INBOX"));
        assert!(!belongs(&labels(&["Label_1", "TRASH"]), "Label_1"));
        assert!(belongs(&labels(&["Label_1", "TRASH"]), "TRASH"));
        assert!(belongs(&labels(&["SPAM"]), "SPAM"));
    }

    #[test]
    fn history_records_fold_into_changed_and_deleted_ids() {
        let records: Vec<GmailHistory> = serde_json::from_str(
            r#"[
                {"id": "1", "messagesAdded": [{"message": {"id": "a"}}]},
                {"id": "2", "labelsAdded": [{"message": {"id": "b"}, "labelIds": ["STARRED"]}]},
                {"id": "3", "labelsRemoved": [{"message": {"id": "a"}, "labelIds": ["UNREAD"]}]},
                {"id": "4", "messagesAdded": [{"message": {"id": "c"}}]},
                {"id": "5", "messagesDeleted": [{"message": {"id": "c"}}]}
            ]"#,
        )
        .unwrap();

        let (changed, deleted) = history_changes(&records);
        assert_eq!(changed, ["a", "b"]);
        assert_eq!(deleted, BTreeSet::from([String::from("c")]));
        assert_eq!(history_added(&records), ["a", "c"]);
    }

    #[test]
    fn a_metadata_get_folds_into_a_summary() {
        let message: GmailMessage = serde_json::from_str(
            r#"{
                "id": "18c",
                "labelIds": ["INBOX", "UNREAD"],
                "sizeEstimate": 2048,
                "payload": {"headers": [
                    {"name": "Message-ID", "value": "<m1@example.org>"},
                    {"name": "In-Reply-To", "value": "<m0@example.org>"},
                    {"name": "Subject", "value": "=?UTF-8?Q?Caf=C3=A9?="},
                    {"name": "From", "value": "Alice <alice@example.org>"},
                    {"name": "To", "value": "bob@example.org, carol@example.org"},
                    {"name": "Date", "value": "Mon, 6 Jul 2026 14:00:00 +0200"}
                ]}
            }"#,
        )
        .unwrap();

        let summary = message_summary("18c", &message);
        assert_eq!(summary.message_id.as_deref(), Some("m1@example.org"));
        assert_eq!(summary.in_reply_to, ["m0@example.org"]);
        assert_eq!(summary.subject, "Café");
        assert_eq!(summary.from[0].name.as_deref(), Some("Alice"));
        assert_eq!(summary.from[0].email, "alice@example.org");
        assert_eq!(summary.to.len(), 2);
        assert_eq!(
            summary
                .date
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-07-06T12:00:00Z"
        );
        assert!(summary.flags.is_empty());
        assert_eq!(summary.size, 2048);
    }

    #[test]
    fn only_a_404_counts_as_not_found() {
        let expired = GmailClientStdError::Send(GmailSendError::Api {
            status: 404,
            message: "Requested entity was not found.".into(),
        });
        assert!(is_not_found(&expired));

        let denied = GmailClientStdError::Send(GmailSendError::Api {
            status: 403,
            message: "no".into(),
        });
        assert!(!is_not_found(&denied));
        assert!(!is_not_found(&GmailClientStdError::Io(
            std::io::Error::other("reset")
        )));
    }

    #[test]
    fn checkpoint_decodes_a_history_id() {
        assert_eq!(decode_checkpoint(b"12345").as_deref(), Some("12345"));
        assert_eq!(decode_checkpoint(&[]), None);
        assert_eq!(decode_checkpoint(&[0xff, 0xfe]), None);
    }

    /// A listed message is named by its metadata: the `Date` header, never
    /// Gmail's `internalDate`, the flags of its labels, and the attachment
    /// mark from the top-level `Content-Type`, both ways.
    #[test]
    fn a_listed_message_is_named_by_its_metadata() {
        let message: GmailMessage = serde_json::from_str(
            r#"{
                "id": "18c",
                "labelIds": ["INBOX", "STARRED"],
                "internalDate": "1791331200000",
                "sizeEstimate": 4096,
                "payload": {"headers": [
                    {"name": "Message-ID", "value": "<m1@example.org>"},
                    {"name": "Date", "value": "Wed, 1 Jan 2020 10:00:00 +0000"},
                    {"name": "Content-Type", "value": "multipart/mixed; boundary=\"x\""}
                ]}
            }"#,
        )
        .unwrap();

        let entry = named_entry("18c", &message);
        assert!(entry.flags.contains(&Flag::from_iana(IanaFlag::Seen)));
        assert!(entry.flags.contains(&Flag::from_iana(IanaFlag::Flagged)));
        let meta = entry.meta.expect("named in the listing");
        assert_eq!(meta.link_id.as_str(), "m1@example.org");
        assert_eq!(
            meta.sort_key.as_str(),
            "2020-01-01T10:00:00Z",
            "the Date header, not the day Gmail received it"
        );
        let Some(io_pimdir::summary::PimdirSummary::Mail(summary)) = meta.summary else {
            panic!("a mail summary");
        };
        assert_eq!(summary.attachment, Some(true));
        assert_eq!(summary.size, Some(4096));

        let plain: GmailMessage = serde_json::from_str(
            r#"{"id": "18d", "payload": {"headers": [
                {"name": "Content-Type", "value": "text/plain; charset=utf-8"}
            ]}}"#,
        )
        .unwrap();
        let meta = named_entry("18d", &plain).meta.unwrap();
        let Some(io_pimdir::summary::PimdirSummary::Mail(summary)) = meta.summary else {
            panic!("a mail summary");
        };
        assert_eq!(summary.attachment, Some(false));
        assert_eq!(summary.date, None);
    }

    /// The search reaches two days around the scope, in epoch seconds Gmail
    /// reads as instants, its own midnights being Pacific.
    #[test]
    fn a_scope_narrows_the_listing_with_a_margin() {
        assert_eq!(scope_query(&PimdirScope::unbounded()), None);
        assert_eq!(
            scope_query(&PimdirScope::since("2026-10-03T00:00:00Z")).as_deref(),
            Some("after:1790812800"),
            "2026-10-01T00:00:00Z"
        );
        let band = PimdirScope {
            since: Some(String::from("2026-10-03T00:00:00Z")),
            until: Some(String::from("2026-10-05T00:00:00Z")),
        };
        assert_eq!(
            scope_query(&band).as_deref(),
            Some("after:1790812800 before:1791331200")
        );
    }

    /// A page token Gmail no longer takes is answered 400.
    #[test]
    fn only_a_400_counts_as_a_refused_page_token() {
        let api = |status| {
            GmailClientStdError::Send(GmailSendError::Api {
                status,
                message: String::from("Invalid pageToken"),
            })
        };
        assert!(is_bad_request(&api(400)));
        assert!(!is_bad_request(&api(404)));
    }
}
