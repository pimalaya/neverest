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
//! for the flags), a delta round projects on the label the mailbox history,
//! read once a run, and an expired history id (HTTP 404) restarts a full
//! round.
//!
//! Messages are read through Gmail's HTTP batch endpoint, a batch of a few
//! reads at a time across the source ([`super::batch`]), paced by the units
//! its reads cost; a message read under one label is not read again under
//! another in the same run ([`super::run`]).
//!
//! Pushes are label changes: a delete removes the collection's label (from
//! `INBOX` it archives), a move swaps two labels, and only a delete from
//! `TRASH` is permanent. A message gone already counts as done. An append
//! goes through `messages.import`, whose answered id is checked before it is
//! trusted.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    io::{self, Read, Write},
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::DateTime;
use io_gmail::v1::{
    batch::{GMAIL_BATCH_URL, GmailBatch, GmailBatchRequest, GmailBatchResponse},
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
    send::{GMAIL_API_BASE, GmailApiError, GmailSendError, GmailSendOutput},
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

use super::{
    batch::{self, Batcher, Read as ReadAs, Sizes},
    run::{GmailRun, History},
};
use crate::{
    account::GmailAccount,
    client::{EnumEntry, Enumeration, Gone, Held, Listed, StoredSize, WrittenItem},
    item::{
        address::Address,
        collection::Collection,
        flag::{Flag, FlagOp, IanaFlag},
        summary::{ItemSummary, normalize_message_id},
    },
    kind::mail,
    offline::invitation::Refusal,
    throttle::{GoogleAnswer, GoogleWaits, Request, Throttle, rfc3339},
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

/// The history records a delta round reads.
const DELTA_HISTORY: &[GmailHistoryType] = &[
    GmailHistoryType::MessageAdded,
    GmailHistoryType::MessageDeleted,
    GmailHistoryType::LabelAdded,
    GmailHistoryType::LabelRemoved,
];

/// The page size of message and history listings, the API maximum.
const PAGE_SIZE: u32 = 500;

/// The ids one page of a round lists (pimdir SYNC §4).
const ROUND_PAGE_SIZE: u32 = 100;

/// How far around a scope the `after:` and `before:` searches reach: Gmail
/// files a message by its reception, which may differ from its `Date`.
const SCOPE_MARGIN_DAYS: i64 = 2;

/// The most ids one `batchModify` takes.
const BATCH_SIZE: usize = 1000;

/// How many times an import's history is read before giving up on it.
const IMPORT_ATTEMPTS: u32 = 3;

/// How long a read on a Gmail stream waits for bytes.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// The quota units a second the Gmail source holds itself to, across all its
/// connections.
///
/// Gmail meters each user at 250 units a second, `messages.list` and
/// `messages.get` costing 5 each: about 50 reads a second, batched or not, a
/// batch counting each request it carries. 200 is 40 reads a second, a margin
/// below the quota rather than a run of refusals at it. The minute's budget
/// ([`super::run::UNITS_PER_MINUTE`]) is what binds a long run.
pub const UNITS_PER_SECOND: f64 = 200.0;

/// What one `messages.get` costs, a batch costing it for each read.
pub const MESSAGE_GET_UNITS: u32 = units::MESSAGE_GET;

/// What each Gmail call costs in quota units, as Gmail bills them.
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

/// The host a request goes to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Host {
    /// `gmail.googleapis.com`, every REST call.
    Api,
    /// `www.googleapis.com`, the batch endpoint.
    Batch,
}

/// The live Gmail session of one side.
pub struct GmailClient {
    inner: GmailClientStd,
    /// The session to the batch endpoint, opened at the first batch.
    batch: Option<GmailClientStd>,
    /// The bearer token, kept to open the batch session.
    token: SecretString,
    /// The TLS configuration, kept for stream reopens.
    tls: Tls,
    /// Label name to label id, refreshed from the label listing when a name
    /// misses.
    labels: HashMap<String, String>,
    /// Whether the server allowed reusing the stream after the last
    /// exchange; when false the next operation reopens it.
    alive: bool,
    /// The same for the batch session.
    batch_alive: bool,
    /// The source's pacing and back-off, shared by its connections.
    throttle: Arc<Throttle>,
    /// What the source's connections share for the run.
    run: Arc<GmailRun>,
    /// The flag sets of the last label and query a round page read, kept
    /// for the run so the next page lists them again for nothing.
    flag_sets: Option<((String, Option<String>), FlagSets)>,
}

impl GmailClient {
    /// Opens the TLS connection to the Gmail API with a bearer token, scoped
    /// to the mailbox owner (`me` or an address).
    pub fn connect(account: &GmailAccount, throttle: Arc<Throttle>) -> Result<Self> {
        let options = GmailClientStdConnectOptions {
            tls: account.tls.clone(),
            proxy: Proxy::None,
            user_id: account.user_id.clone(),
        };
        let inner = GmailClientStd::connect(account.token.expose_secret(), options)
            .context("Cannot connect to Gmail")?;

        Ok(Self {
            inner,
            batch: None,
            token: account.token.clone(),
            tls: account.tls.clone(),
            labels: HashMap::new(),
            alive: true,
            batch_alive: false,
            throttle,
            run: Arc::clone(&account.run),
            flag_sets: None,
        })
    }

    /// Reopens the stream to the Gmail endpoint, keeping the credential.
    fn reconnect(&mut self) -> Result<()> {
        debug!("reopening the gmail stream");
        let stream = open_stream(&self.tls, GMAIL_API_BASE)?;
        self.inner.set_stream(stream);
        self.alive = true;
        Ok(())
    }

    /// The session to the batch endpoint, opened or reopened when the
    /// server closed it.
    fn batch_session(&mut self) -> Result<&mut GmailClientStd, GmailClientStdError> {
        if !self.batch_alive || self.batch.is_none() {
            debug!("opening the gmail batch stream");
            let stream = open_stream(&self.tls, GMAIL_BATCH_URL)
                .map_err(|err| GmailClientStdError::Io(io::Error::other(format!("{err:#}"))))?;
            match &mut self.batch {
                Some(session) => session.set_stream(stream),
                None => {
                    let options = GmailClientStdConnectOptions {
                        tls: self.tls.clone(),
                        proxy: Proxy::None,
                        user_id: self.inner.user_id.clone(),
                    };
                    let token = self.token.expose_secret();
                    self.batch = Some(GmailClientStd::new(stream, token, options));
                }
            }
            self.batch_alive = true;
        }
        self.batch
            .as_mut()
            .ok_or_else(|| GmailClientStdError::Io(io::Error::other("no gmail batch session")))
    }

    /// Runs one Gmail operation costing `units` of quota, reopening the
    /// stream first when the server closed it, and records the new
    /// keep-alive hint.
    ///
    /// The source's [`Throttle`] paces it and sends it again while Gmail
    /// throttles it (429, 503, or a 403 rate limit), by the reason Google
    /// gives ([`GoogleWaits`]). A request creating something goes through
    /// [`create`](Self::create) instead.
    fn op<T>(
        &mut self,
        units: u32,
        run: impl FnMut(&mut GmailClientStd) -> Result<GmailSendOutput<T>, GmailClientStdError>,
    ) -> Result<T, GmailClientStdError> {
        self.send(Host::Api, Request::Idempotent, units, run)
    }

    /// Runs one Gmail request that creates something (a label, an imported
    /// message): sent again on a refusal Gmail answers before doing anything
    /// (429, a rate limit), never on a 503, after which it may exist.
    fn create<T>(
        &mut self,
        units: u32,
        run: impl FnMut(&mut GmailClientStd) -> Result<GmailSendOutput<T>, GmailClientStdError>,
    ) -> Result<T, GmailClientStdError> {
        self.send(Host::Api, Request::Create, units, run)
    }

    /// Runs one Gmail request of `request` on `host`, as [`op`](Self::op)
    /// describes.
    fn send<T>(
        &mut self,
        host: Host,
        request: Request,
        units: u32,
        mut run: impl FnMut(&mut GmailClientStd) -> Result<GmailSendOutput<T>, GmailClientStdError>,
    ) -> Result<T, GmailClientStdError> {
        let throttle = Arc::clone(&self.throttle);
        let waits = GoogleWaits::new();
        throttle.call(
            units,
            || {
                let session = match host {
                    Host::Api => {
                        if !self.alive
                            && let Err(err) = self.reconnect()
                        {
                            warn!("cannot reopen the gmail stream: {err:#}");
                        }
                        &mut self.inner
                    }
                    Host::Batch => self.batch_session()?,
                };

                let out = run(session);
                let alive = match &out {
                    Ok(out) => out.keep_alive,
                    // NOTE: a stream that failed mid-exchange is in no state
                    // to carry the next one.
                    Err(GmailClientStdError::Io(_)) => false,
                    Err(_) => true,
                };
                match host {
                    Host::Api => self.alive = alive,
                    Host::Batch => self.batch_alive = alive,
                }
                Ok(out?.response)
            },
            |err| match err {
                GmailClientStdError::Send(GmailSendError::Api(err)) => {
                    waits.throttled(request, &google_answer(err))
                }
                _ => None,
            },
            |message| {
                GmailClientStdError::Send(GmailSendError::Api(GmailApiError {
                    status: 429,
                    message,
                    ..Default::default()
                }))
            },
        )
    }

    /// Reads `ids` as `read` through batches ([`batch::read_all`]), handing
    /// each to `each` once, `None` for a message gone since it was listed.
    fn read_messages(
        &mut self,
        read: ReadAs,
        ids: &[String],
        sizes: Sizes<'_>,
        each: impl FnMut(String, Option<GmailMessage>) -> Result<()>,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let batch_size = self.run.settings.batch_size;
        let mut batcher = LiveBatcher { client: self };
        batch::read_all(&mut batcher, read, ids, batch_size, sizes, each)
    }

    /// The label ids synced as collections: a message carrying two of them
    /// is listed twice.
    fn collection_ids(&self) -> HashSet<String> {
        self.labels.values().cloned().collect()
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
    /// A delta projects on the label the mailbox history since the
    /// checkpoint's history id, read once a run, an expired one (HTTP 404)
    /// being a rejected checkpoint. A round lists the label newest first,
    /// [`ROUND_PAGE_SIZE`] ids a page under the scope's `after:` (two days
    /// below `since`, a superset of the scope on `Date`), its cursor the
    /// page token; its first page carries the profile's history id, read
    /// before the listing, so what lands during the round is the next
    /// delta's.
    pub fn enumerate(
        &mut self,
        collection: &str,
        request: &PimdirEnumerate,
        held: Held<'_>,
    ) -> Result<Listed> {
        let label = self.label_id(collection)?;

        match &request.listing {
            // NOTE: no head yet (pimdir SYNC §4).
            PimdirListing::Head(_) => Ok(Listed::NoHead),
            PimdirListing::Delta(checkpoint) => {
                let Some(start) = decode_checkpoint(&checkpoint.0) else {
                    return Ok(Listed::CursorRejected);
                };
                match self.changes(&label, &start, &held) {
                    Ok(changes) => self
                        .delta_round(collection, &label, changes, held)
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
    ///
    /// The members the store binds are named by their binding and flagged
    /// from the label's flag listings; the others by their metadata, read in
    /// batches, or kept from another label's listing this run, then flagged
    /// as the bound ones are.
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

        let run = Arc::clone(&self.run);
        let mut bound = Vec::new();
        let mut kept = Vec::new();
        let mut unread = Vec::new();
        for message in page.messages {
            let id = message.id;
            if held.holds(&id) {
                bound.push(id);
            } else if let Some(message) = run.seen(&id, true, None) {
                kept.push((id, message));
            } else {
                unread.push(id);
            }
        }

        let mut items = Vec::with_capacity(bound.len() + kept.len() + unread.len());
        // NOTE: a message kept from another label's listing names itself by
        // its headers, but its flags may have moved since: they are read
        // from this label's listings, as a bound one's are.
        if !bound.is_empty() || !kept.is_empty() {
            let flags = self.flag_sets(label, query.as_deref())?;
            items.extend(bound.into_iter().map(|id| {
                let flags = flags.of(&id);
                EnumEntry::bare(id, flags, None)
            }));
            items.extend(kept.into_iter().map(|(id, message)| {
                let mut entry = named_entry(&id, &message);
                entry.flags = flags.of(&id);
                entry
            }));
        }

        let shared = self.collection_ids();
        let mut bytes = 0;
        let at = Instant::now();
        self.read_messages(ReadAs::Metadata, &unread, &|_| 0, |id, message| {
            // NOTE: gone since the listing: absent from the round, as it is
            // from the label.
            let Some(message) = message else {
                return Ok(());
            };
            bytes += serde_json::to_vec(&message).map_or(0, |json| json.len() as u64);
            keep(&run, &shared, &id, &message, true, at);
            items.push(named_entry(&id, &message));
            Ok(())
        })
        .with_context(|| format!("Read the metadata of {collection} error"))?;

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

    /// Reads the history since `start`, paged, returning the records and
    /// the mailbox history id the next round starts from: the mailbox's
    /// whole history, or `label`'s.
    ///
    /// The error comes back unwrapped, so the caller can tell an expired
    /// history id (404) from any other failure.
    fn history(
        &mut self,
        label: Option<&str>,
        start: &str,
        types: &[GmailHistoryType],
    ) -> Result<(Vec<GmailHistory>, String), GmailClientStdError> {
        let mut records = Vec::new();
        let mut checkpoint = start.to_owned();
        let mut token: Option<String> = None;
        loop {
            let params = GmailHistoryListParams {
                start_history_id: start,
                label_id: label,
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

    /// The changes of `label` since `start`, projected from the mailbox
    /// history the run reads once ([`GmailRun::history`]).
    fn changes(
        &mut self,
        label: &str,
        start: &str,
        held: &Held<'_>,
    ) -> Result<Changes, GmailClientStdError> {
        let run = Arc::clone(&self.run);
        run.history(
            start,
            || {
                debug!("read the gmail history since {start}");
                let at = Instant::now();
                let (records, next) = self.history(None, start, DELTA_HISTORY)?;
                Ok(History::new(start, at, &records, next))
            },
            |history| {
                let (changed, deleted) = history.project(label, start, |id| held.holds(id));
                Changes {
                    changed,
                    deleted,
                    next: history.next.clone(),
                    since: history.at,
                }
            },
        )
    }

    /// Folds a label's changes into a delta.
    ///
    /// History records are increments, not state, so every changed message's
    /// current labels are read before it is reported, in batches: its labels
    /// alone for one the store binds, its metadata for one it does not, which
    /// names it, unless another label read it since the history was. Still
    /// carrying the label, it is an item with its flags; otherwise or gone,
    /// it vanished.
    fn delta_round(
        &mut self,
        collection: &str,
        label: &str,
        changes: Changes,
        held: Held<'_>,
    ) -> Result<Enumeration> {
        debug!("begin gmail delta round");
        trace!(
            "collection: {collection}, changed: {}, deleted: {}",
            changes.changed.len(),
            changes.deleted.len()
        );

        let run = Arc::clone(&self.run);
        let mut items = Vec::new();
        let mut vanished = changes.deleted;
        let mut labels_only = Vec::new();
        let mut metadata = Vec::new();
        for id in changes.changed {
            let bound = held.holds(&id);
            match run.seen(&id, !bound, Some(changes.since)) {
                Some(message) => settle(label, id, bound, &message, &mut items, &mut vanished),
                None if bound => labels_only.push(id),
                None => metadata.push(id),
            }
        }

        let shared = self.collection_ids();
        let mut bytes = 0;
        for (read, ids) in [(ReadAs::Minimal, labels_only), (ReadAs::Metadata, metadata)] {
            let at = Instant::now();
            let bound = read == ReadAs::Minimal;
            self.read_messages(read, &ids, &|_| 0, |id, message| {
                let Some(message) = message else {
                    vanished.push(id);
                    return Ok(());
                };
                if !bound {
                    bytes += serde_json::to_vec(&message).map_or(0, |json| json.len() as u64);
                }
                keep(&run, &shared, &id, &message, !bound, at);
                settle(label, id, bound, &message, &mut items, &mut vanished);
                Ok(())
            })
            .with_context(|| format!("Read the changes of {collection} error"))?;
        }

        debug!("end of gmail delta round");
        trace!("items: {}, vanished: {}", items.len(), vanished.len());

        let mut delta = Enumeration::delta(items, vanished, changes.next.into_bytes());
        delta.bytes = bytes;
        Ok(delta)
    }

    /// Fetches the summaries of an id set, by batches of metadata reads; a
    /// message gone is left out.
    pub fn fetch_summaries(&mut self, ids: &[&str]) -> Result<Vec<ItemSummary>> {
        let ids: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let run = Arc::clone(&self.run);
        let shared = self.collection_ids();
        let at = Instant::now();
        let mut summaries = Vec::with_capacity(ids.len());
        self.read_messages(ReadAs::Metadata, &ids, &|_| 0, |id, message| {
            if let Some(message) = message {
                keep(&run, &shared, &id, &message, true, at);
                summaries.push(message_summary(&id, &message));
            }
            Ok(())
        })
        .context("Read message metadata error")?;
        Ok(summaries)
    }

    /// Streams the bodies of an id set, by batches of raw reads, each body
    /// written to its sink as its batch lands, and answers the ids gone
    /// since they were listed.
    ///
    /// A batch holds about [`batch::RAW_BATCH_BYTES`] by the sizes the run
    /// read with the metadata, else those the store records (`stored`), a
    /// message past [`batch::ALONE_BYTES`] being read alone; a size neither
    /// knows comes from a minimal read first.
    pub fn fetch_bodies<S: Write>(
        &mut self,
        ids: &[&str],
        stored: StoredSize<'_>,
        mut open: impl FnMut(&str) -> std::io::Result<S>,
        mut done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<Vec<String>> {
        let run = Arc::clone(&self.run);
        let mut sizes: HashMap<String, u64> = HashMap::with_capacity(ids.len());
        let mut unknown = Vec::new();
        for id in ids {
            // NOTE: the run's own read first, which also drops it from the
            // run's cache, then the store's, a resumed hydration's.
            match run
                .take_size(id)
                .or_else(|| stored(id).filter(|size| *size > 0))
            {
                Some(size) => {
                    sizes.insert(id.to_string(), size);
                }
                None => unknown.push(id.to_string()),
            }
        }

        let mut gone = Vec::new();
        self.read_messages(ReadAs::Minimal, &unknown, &|_| 0, |id, message| {
            match message {
                Some(message) => {
                    sizes.insert(id, message.size_estimate.unwrap_or(0));
                }
                None => gone.push(id),
            }
            Ok(())
        })
        .context("Read message sizes error")?;

        let ids: Vec<String> = ids
            .iter()
            .filter(|id| !gone.iter().any(|gone| gone == **id))
            .map(|id| id.to_string())
            .collect();
        let size = |id: &str| sizes.get(id).copied().unwrap_or(0);
        self.read_messages(ReadAs::Raw, &ids, &size, |id, message| {
            let Some(message) = message else {
                debug!("message {id} gone before its body was read");
                gone.push(id);
                return Ok(());
            };
            let raw = raw_body(&id, message)?;
            let mut sink = open(&id).with_context(|| format!("Open body sink for {id} error"))?;
            sink.write_all(&raw)
                .with_context(|| format!("Store body {id} error"))?;
            drop(raw);
            done(&id, None, sink).with_context(|| format!("Commit body {id} error"))?;
            Ok(())
        })
        .context("Read message bodies error")?;
        Ok(gone)
    }

    /// Streams one message's raw RFC 5322 bytes into `sink`; a message
    /// gone fails with [`Gone`].
    pub fn get_item_stream(&mut self, id: &str, mut sink: impl Write) -> Result<Option<String>> {
        let raw = self.message_raw(id)?;
        sink.write_all(&raw)
            .with_context(|| format!("Stream body {id} error"))?;
        Ok(None)
    }

    /// Fetches the raw RFC 5322 bytes of one message, base64url-decoded.
    fn message_raw(&mut self, id: &str) -> Result<Vec<u8>> {
        let message = match self.op(units::MESSAGE_GET, |client| {
            client.message_get(id, GmailMessageFormat::Raw, &[])
        }) {
            Ok(message) => message,
            Err(err) if is_not_found(&err) => {
                return Err(anyhow::Error::new(err).context(Gone(id.to_owned())));
            }
            Err(err) => {
                return Err(anyhow::Error::new(err).context(format!("Get raw message {id} error")));
            }
        };
        raw_body(id, message)
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
        drop(raw);
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
                .history(Some(&label), &start, &[GmailHistoryType::MessageAdded])
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
    /// archives when it is `INBOX`; from `TRASH` it is deleted for good. A
    /// message gone already is deleted.
    pub fn delete_item(&mut self, collection: &str, id: &str) -> Result<()> {
        let label = self.label_id(collection)?;

        if label == TRASH {
            match self.op(units::MESSAGE_DELETE, |client| client.message_delete(id)) {
                Ok(_) => {}
                Err(err) if is_not_found(&err) => debug!("message {id} already gone"),
                Err(err) => {
                    return Err(
                        anyhow::Error::new(err).context(format!("Delete message {id} error"))
                    );
                }
            }
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
    ///
    /// A message gone already has nothing left to change, which counts as
    /// done: a single modify answered 404 succeeds, and a chunk answered
    /// 404 is modified again one message at a time, each gone one skipped.
    fn modify(&mut self, ids: &[&str], add: &[String], remove: &[String]) -> Result<()> {
        if let [id] = ids {
            return self.modify_one(id, add, remove);
        }

        for chunk in ids.chunks(BATCH_SIZE) {
            let owned: Vec<String> = chunk.iter().map(|id| id.to_string()).collect();
            let modified = self.op(units::BATCH_MODIFY, |client| {
                let coroutine = GmailMessagesBatchModify::new(
                    &client.auth,
                    &client.user_id,
                    &owned,
                    add,
                    remove,
                )?;
                client.run(coroutine)
            });
            match modified {
                Ok(_) => {}
                Err(err) if is_not_found(&err) => {
                    debug!("batch modify answered 404, modifying its messages one by one");
                    for id in chunk {
                        self.modify_one(id, add, remove)?;
                    }
                }
                Err(err) => return Err(err.into()),
            }
        }

        Ok(())
    }

    /// Adds and removes labels on one message, gone already counting as
    /// done.
    fn modify_one(&mut self, id: &str, add: &[String], remove: &[String]) -> Result<()> {
        match self.op(units::MESSAGE_MODIFY, |client| {
            client.message_modify(id, add, remove)
        }) {
            Ok(_) => Ok(()),
            Err(err) if is_not_found(&err) => {
                debug!("message {id} already gone, nothing to modify");
                Ok(())
            }
            Err(err) => Err(err.into()),
        }
    }
}

/// The batch sender over a live Gmail session ([`Batcher`]).
struct LiveBatcher<'c> {
    client: &'c mut GmailClient,
}

impl Batcher for LiveBatcher<'_> {
    /// One batch to the batch endpoint, through the source's gate (one
    /// batch in flight by default) and paced by the units of all its reads.
    fn batch(
        &mut self,
        ids: &[String],
        read: ReadAs,
    ) -> Result<Vec<GmailBatchResponse<GmailMessage>>> {
        let user = self.client.inner.user_id.clone();
        // NOTE: each read is tagged by its index, whatever its id holds.
        let requests = ids
            .iter()
            .enumerate()
            .map(|(n, id)| {
                GmailBatchRequest::message_get(n, &user, id, read.format(), read.headers())
            })
            .collect::<Result<Vec<_>, _>>()
            .context("Build Gmail batch error")?;
        let units = units::MESSAGE_GET.saturating_mul(ids.len() as u32);

        let run = Arc::clone(&self.client.run);
        let pass = run.enter();
        let answers = self
            .client
            .send(Host::Batch, Request::Idempotent, units, |client| {
                let coroutine = GmailBatch::<GmailMessage>::new(&client.auth, &requests)?;
                client.run(coroutine)
            })
            .context("Send Gmail batch error")?;
        drop(pass);

        Ok(answers
            .into_iter()
            .map(|mut answer| {
                if let Some(id) = answer.id.parse::<usize>().ok().and_then(|n| ids.get(n)) {
                    answer.id = id.clone();
                }
                answer
            })
            .collect())
    }

    fn single(&mut self, id: &str, read: ReadAs) -> Result<Option<GmailMessage>> {
        match self.client.op(units::MESSAGE_GET, |client| {
            client.message_get(id, read.format(), read.headers())
        }) {
            Ok(message) => Ok(Some(message)),
            Err(err) if is_not_found(&err) => Ok(None),
            Err(err) => Err(anyhow::Error::new(err).context(format!("Get message {id} error"))),
        }
    }

    fn pause(&mut self, wait: Duration) -> Result<()> {
        let throttle = &self.client.throttle;
        if let Some(until) = throttle.blocked() {
            bail!(
                "Gmail gave up after being throttled, until {}",
                rfc3339(until)
            );
        }
        if !throttle.waits_in_process(wait) {
            let until = SystemTime::now() + wait;
            throttle.give_up(until);
            bail!("Gmail throttled, giving up until {}", rfc3339(until));
        }
        debug!("gmail batch paused {} ms", wait.as_millis());
        thread::sleep(wait);
        Ok(())
    }

    fn give_up(&mut self, until: SystemTime) {
        warn!("gmail throttled, giving up until {}", rfc3339(until));
        self.client.throttle.give_up(until);
    }
}

/// A label's changes since a checkpoint, projected from the run's history.
struct Changes {
    /// The ids to read again.
    changed: Vec<String>,
    /// The ids deleted for good.
    deleted: Vec<String>,
    /// The checkpoint the next delta starts from.
    next: String,
    /// When the history was read: a message read before is no evidence.
    since: Instant,
}

/// Opens a TLS stream to the host of `url`.
fn open_stream(tls: &Tls, url: &str) -> Result<Stream> {
    let url = Url::parse(url).with_context(|| format!("Cannot parse {url}"))?;
    let host = url
        .host_str()
        .with_context(|| format!("{url} has no host"))?;
    let opts = TlsConnectOptions {
        tls: tls.clone(),
        ..Default::default()
    };
    let stream = Stream::connect_tls(host, url.port().unwrap_or(443), opts)?;
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    Ok(stream)
}

/// A Gmail error as the throttle reads it.
fn google_answer(err: &GmailApiError) -> GoogleAnswer<'_> {
    GoogleAnswer {
        status: err.status,
        message: &err.message,
        reasons: &err.reasons,
        detail_reasons: &err.detail_reasons,
        google_status: err.google_status.as_deref(),
    }
}

/// A raw read's bytes, its base64 dropped as soon as it is decoded.
fn raw_body(id: &str, mut message: GmailMessage) -> Result<Vec<u8>> {
    let raw = message
        .raw
        .take()
        .with_context(|| format!("Raw message {id} carries no body"))?;
    drop(message);
    decode_raw(&raw).map_err(|err| anyhow!("Decode raw message {id} error: {err}"))
}

/// Keeps a message read at `at` for the rest of the run: whole when it
/// carries another label synced as a collection, which will list it too,
/// else its size alone, for its body read.
fn keep(
    run: &GmailRun,
    shared: &HashSet<String>,
    id: &str,
    message: &GmailMessage,
    metadata: bool,
    at: Instant,
) {
    let collections = message
        .label_ids
        .iter()
        .filter(|label| shared.contains(*label))
        .count();
    if collections > 1 {
        let mut message = message.clone();
        if message.id.is_empty() {
            message.id = id.to_owned();
        }
        run.keep(&message, metadata, at);
    } else if let Some(size) = message.size_estimate {
        run.keep_size(id, size);
    }
}

/// Settles a changed message of a delta: an item of `label` while it still
/// carries the label, named by its binding when `bound`, else vanished.
fn settle(
    label: &str,
    id: String,
    bound: bool,
    message: &GmailMessage,
    items: &mut Vec<EnumEntry>,
    vanished: &mut Vec<String>,
) {
    if !belongs(&message.label_ids, label) {
        vanished.push(id);
    } else if bound {
        let flags = flags_from_labels(&message.label_ids);
        items.push(EnumEntry::bare(id, flags, None));
    } else {
        items.push(named_entry(&id, message));
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

/// A Gmail client over scripted sessions, for the tests here and the
/// driver's.
#[cfg(test)]
pub(crate) mod scripted {
    use std::io::{self, Read, Write};

    use super::*;

    /// A scripted Gmail session: each answer served whole to the read that
    /// follows a request, each request's first line recorded.
    #[derive(Clone, Default)]
    pub(crate) struct Wire {
        answers: Arc<std::sync::Mutex<std::collections::VecDeque<Vec<u8>>>>,
        requests: Arc<std::sync::Mutex<Vec<String>>>,
        current: Vec<u8>,
        at: usize,
    }

    impl Wire {
        pub(crate) fn answer(&self, answer: Vec<u8>) {
            self.answers.lock().unwrap().push_back(answer);
        }

        pub(crate) fn requests(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }

        pub(crate) fn left(&self) -> usize {
            self.answers.lock().unwrap().len()
        }
    }

    impl Read for Wire {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.at == self.current.len() {
                self.current = self.answers.lock().unwrap().pop_front().unwrap_or_default();
                self.at = 0;
            }
            let n = buf.len().min(self.current.len() - self.at);
            buf[..n].copy_from_slice(&self.current[self.at..self.at + n]);
            self.at += n;
            Ok(n)
        }
    }

    impl Write for Wire {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let text = String::from_utf8_lossy(buf);
            let line = text.lines().next().unwrap_or_default();
            if ["GET ", "POST ", "DELETE ", "PUT ", "PATCH "]
                .iter()
                .any(|method| line.starts_with(method))
            {
                self.requests.lock().unwrap().push(line.to_owned());
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    pub(crate) fn json(status: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    pub(crate) fn ok(body: serde_json::Value) -> Vec<u8> {
        json("200 OK", &body.to_string())
    }

    /// A batch answer, its parts tagged by the index of their read.
    pub(crate) fn batch_answer(parts: &[(usize, &str, serde_json::Value)]) -> Vec<u8> {
        let mut body = String::new();
        for (n, status, json) in parts {
            body.push_str("--b\r\nContent-Type: application/http\r\n");
            body.push_str(&format!("Content-ID: <response-{n}>\r\n\r\n"));
            body.push_str(&format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\r\n{json}\r\n"
            ));
        }
        body.push_str("--b--\r\n");
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: multipart/mixed; boundary=b\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    /// A client over two scripted wires, the API's and the batch
    /// endpoint's, knowing the labels `INBOX` and `Work` (`Label_1`).
    pub(crate) fn scripted() -> (GmailClient, Wire, Wire) {
        let (api, batch) = (Wire::default(), Wire::default());
        let options = || GmailClientStdConnectOptions {
            tls: Tls::default(),
            proxy: Proxy::None,
            user_id: String::from("me"),
        };
        let client = GmailClient {
            inner: GmailClientStd::new(api.clone(), "token", options()),
            batch: Some(GmailClientStd::new(batch.clone(), "token", options())),
            token: SecretString::from(String::from("token")),
            tls: Tls::default(),
            labels: HashMap::from([
                (String::from("INBOX"), String::from("INBOX")),
                (String::from("Work"), String::from("Label_1")),
                (String::from("TRASH"), String::from("TRASH")),
            ]),
            alive: true,
            batch_alive: true,
            throttle: Arc::new(Throttle::new("gmail")),
            run: Arc::new(GmailRun::new(Default::default())),
            flag_sets: None,
        };
        (client, api, batch)
    }
}

#[cfg(test)]
mod tests {
    use io_gmail::v1::send::{GmailApiError, GmailSendError};

    use super::scripted::*;
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
    fn a_history_names_the_messages_it_added_once() {
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
        let expired = GmailClientStdError::Send(GmailSendError::Api(GmailApiError {
            status: 404,
            message: "Requested entity was not found.".into(),
            ..Default::default()
        }));
        assert!(is_not_found(&expired));

        let denied = GmailClientStdError::Send(GmailSendError::Api(GmailApiError {
            status: 403,
            message: "no".into(),
            ..Default::default()
        }));
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
            GmailClientStdError::Send(GmailSendError::Api(GmailApiError {
                status,
                message: String::from("Invalid pageToken"),
                ..Default::default()
            }))
        };
        assert!(is_bad_request(&api(400)));
        assert!(!is_bad_request(&api(404)));
    }

    fn metadata(id: &str, labels: &[&str]) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "labelIds": labels,
            "sizeEstimate": 1000,
            "payload": {"headers": [
                {"name": "Message-ID", "value": format!("<{id}@example.org>")},
                {"name": "Date", "value": "Mon, 6 Jul 2026 14:00:00 +0200"},
            ]}
        })
    }

    fn listed(ids: &[&str]) -> Vec<u8> {
        let messages: Vec<_> = ids
            .iter()
            .map(|id| serde_json::json!({ "id": id }))
            .collect();
        ok(serde_json::json!({ "messages": messages }))
    }

    fn round() -> PimdirEnumerate {
        PimdirEnumerate {
            listing: PimdirListing::Round {
                cursor: None,
                band: false,
            },
            scope: PimdirScope::unbounded(),
        }
    }

    fn delta(from: &str) -> PimdirEnumerate {
        PimdirEnumerate {
            listing: PimdirListing::Delta(io_pimdir::collection::PimdirCheckpoint(
                from.as_bytes().to_vec(),
            )),
            scope: PimdirScope::unbounded(),
        }
    }

    fn held<'a>(handles: &'a HashSet<String>) -> Held<'a> {
        Held {
            handles,
            checkpoint: None,
            coverage: None,
        }
    }

    fn page(listed: Listed) -> Enumeration {
        match listed {
            Listed::Page(page) => page,
            Listed::CursorRejected | Listed::NoHead | Listed::HeadRejected => panic!("a page"),
        }
    }

    /// A message under two labels is read once a run: the second label's
    /// listing names it from what the first read, its flags from its own
    /// flag listings.
    #[test]
    fn a_message_under_two_labels_is_read_once() {
        let (mut client, api, batch) = scripted();
        let none = HashSet::new();

        api.answer(ok(
            serde_json::json!({ "emailAddress": "user@example.org", "historyId": "500" }),
        ));
        api.answer(listed(&["m1", "m2"]));
        batch.answer(batch_answer(&[
            (0, "200 OK", metadata("m1", &["INBOX", "Label_1", "UNREAD"])),
            (1, "200 OK", metadata("m2", &["INBOX"])),
        ]));
        let inbox = page(client.enumerate("INBOX", &round(), held(&none)).unwrap());
        assert_eq!(inbox.items.len(), 2);

        api.answer(ok(
            serde_json::json!({ "emailAddress": "user@example.org", "historyId": "501" }),
        ));
        api.answer(listed(&["m1"]));
        // NOTE: read since, by another client.
        api.answer(listed(&[]));
        api.answer(listed(&["m1"]));
        api.answer(listed(&[]));
        let work = page(client.enumerate("Work", &round(), held(&none)).unwrap());

        assert_eq!(batch.requests().len(), 1, "one batch in all");
        assert!(batch.requests()[0].starts_with("POST /batch/gmail/v1 "));
        let [entry] = &work.items[..] else {
            panic!("one member");
        };
        assert!(entry.meta.is_some(), "named");
        assert_eq!(
            entry.flags,
            BTreeSet::from([
                Flag::from_iana(IanaFlag::Seen),
                Flag::from_iana(IanaFlag::Flagged)
            ]),
            "the flags of this listing, not of the first read"
        );
        assert_eq!(work.bytes, 0, "nothing downloaded for it");
        assert_eq!(api.left() + batch.left(), 0);
    }

    /// The mailbox history is read once a run, with no label, and each
    /// label takes its changes from it.
    #[test]
    fn the_history_is_read_once_and_projected_on_each_label() {
        let (mut client, api, batch) = scripted();
        let m1 = HashSet::from([String::from("m1")]);

        api.answer(ok(serde_json::json!({
            "history": [
                { "id": "110", "labelsAdded": [{
                    "message": { "id": "m1", "labelIds": ["INBOX", "Label_1", "STARRED"] },
                    "labelIds": ["STARRED"]
                }]},
                { "id": "111", "messagesAdded": [{
                    "message": { "id": "m3", "labelIds": ["Label_1"] }
                }]},
            ],
            "historyId": "120"
        })));
        batch.answer(batch_answer(&[(
            0,
            "200 OK",
            serde_json::json!({ "id": "m1", "labelIds": ["INBOX", "Label_1", "STARRED"] }),
        )]));
        let inbox = page(client.enumerate("INBOX", &delta("100"), held(&m1)).unwrap());
        assert_eq!(inbox.checkpoint.as_deref(), Some(&b"120"[..]));
        let ids: Vec<&str> = inbox.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids, ["m1"], "m3 is not the inbox's");

        batch.answer(batch_answer(&[(0, "200 OK", metadata("m3", &["Label_1"]))]));
        let work = page(client.enumerate("Work", &delta("100"), held(&m1)).unwrap());
        assert_eq!(work.checkpoint.as_deref(), Some(&b"120"[..]));
        let mut ids: Vec<&str> = work.items.iter().map(|item| item.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["m1", "m3"]);

        let history: Vec<String> = api
            .requests()
            .into_iter()
            .filter(|line| line.contains("/history"))
            .collect();
        assert_eq!(history.len(), 1, "{history:?}");
        assert!(!history[0].contains("labelId"), "{}", history[0]);
        assert_eq!(batch.requests().len(), 2, "m1 read once, m3 once");
        assert_eq!(api.left() + batch.left(), 0);
    }

    /// An expired history restarts a round, for that label only.
    #[test]
    fn an_expired_history_rejects_the_checkpoint() {
        let (mut client, api, _) = scripted();
        let none = HashSet::new();
        api.answer(json(
            "404 Not Found",
            r#"{"error":{"code":404,"message":"Requested entity was not found.","status":"NOT_FOUND"}}"#,
        ));
        let listed = client.enumerate("INBOX", &delta("1"), held(&none)).unwrap();
        assert!(matches!(listed, Listed::CursorRejected));
    }

    /// Bodies come by raw batches, sized by the run's reads, a size it did
    /// not read asked first; a message gone is left out.
    #[test]
    fn bodies_come_by_batches_sized_by_what_the_run_read() {
        let (mut client, _, batch) = scripted();
        client.run.keep_size("m1", 100);

        batch.answer(batch_answer(&[
            (
                0,
                "200 OK",
                serde_json::json!({ "id": "m4", "sizeEstimate": 200 }),
            ),
            (
                1,
                "404 Not Found",
                serde_json::json!({ "error": { "code": 404 } }),
            ),
        ]));
        let raw = |id: &str| {
            let source = format!("Message-ID: <{id}@example.org>\r\n\r\nBody of {id}\r\n");
            serde_json::json!({ "id": id, "raw": encode_raw(source.as_bytes()) })
        };
        batch.answer(batch_answer(&[
            (0, "200 OK", raw("m1")),
            (1, "200 OK", raw("m4")),
        ]));

        let mut bodies = BTreeMap::new();
        let gone = client
            .fetch_bodies(
                &["m1", "m4", "m5"],
                &|_| None,
                |_| Ok(Vec::new()),
                |id, _, sink| {
                    bodies.insert(id.to_owned(), String::from_utf8(sink).unwrap());
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(batch.requests().len(), 2, "the sizes, then the bodies");
        assert_eq!(bodies.len(), 2, "m5 is gone");
        assert_eq!(gone, ["m5"], "and answered gone");
        assert!(bodies["m4"].ends_with("Body of m4\r\n"));
        assert_eq!(batch.left(), 0);
    }

    fn raw(id: &str) -> serde_json::Value {
        let source = format!("Message-ID: <{id}@example.org>\r\n\r\nBody of {id}\r\n");
        serde_json::json!({ "id": id, "raw": encode_raw(source.as_bytes()) })
    }

    /// A resumed hydration plans its batches by the sizes the store
    /// records: no minimal read asks for them first.
    #[test]
    fn the_sizes_the_store_records_spare_a_minimal_read() {
        let (mut client, api, batch) = scripted();
        batch.answer(batch_answer(&[
            (0, "200 OK", raw("m1")),
            (1, "200 OK", raw("m2")),
        ]));

        let stored = |id: &str| match id {
            "m1" => Some(100),
            "m2" => Some(200),
            _ => None,
        };
        let mut bodies = Vec::new();
        let gone = client
            .fetch_bodies(
                &["m1", "m2"],
                &stored,
                |_| Ok(Vec::new()),
                |id, _, _| {
                    bodies.push(id.to_owned());
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(bodies, ["m1", "m2"]);
        assert!(gone.is_empty());
        let requests = batch.requests();
        assert_eq!(requests.len(), 1, "the bodies alone: {requests:?}");
        assert_eq!(api.left() + batch.left(), 0);
    }

    /// A size the store records past what a batch holds reads that
    /// message alone, without asking Gmail its size.
    #[test]
    fn a_large_size_the_store_records_reads_its_message_alone() {
        let (mut client, api, batch) = scripted();
        api.answer(ok(raw("big")));
        batch.answer(batch_answer(&[(0, "200 OK", raw("small"))]));

        let stored = |id: &str| match id {
            "big" => Some(batch::ALONE_BYTES + 1),
            _ => Some(10),
        };
        let mut bodies = Vec::new();
        client
            .fetch_bodies(
                &["big", "small"],
                &stored,
                |_| Ok(Vec::new()),
                |id, _, _| {
                    bodies.push(id.to_owned());
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(bodies, ["big", "small"]);
        let alone = api.requests();
        assert_eq!(alone.len(), 1, "{alone:?}");
        assert!(alone[0].contains("/messages/big"), "{alone:?}");
        assert!(alone[0].contains("format=RAW"), "{alone:?}");
        assert_eq!(batch.requests().len(), 1, "the small one by a batch");
        assert_eq!(api.left() + batch.left(), 0);
    }

    /// A message read alone and answered 404 is gone, not an error.
    #[test]
    fn a_message_gone_alone_is_gone() {
        let (mut client, api, _) = scripted();
        api.answer(json(
            "404 Not Found",
            r#"{"error":{"code":404,"message":"Requested entity was not found.","status":"NOT_FOUND"}}"#,
        ));
        let err = client.get_item_stream("m9", Vec::new()).unwrap_err();
        assert!(Gone::is(&err), "{err:#}");

        api.answer(json(
            "403 Forbidden",
            r#"{"error":{"code":403,"message":"Insufficient Permission"}}"#,
        ));
        let err = client.get_item_stream("m9", Vec::new()).unwrap_err();
        assert!(!Gone::is(&err), "another refusal is no gone: {err:#}");
    }

    /// A write on a message gone already has nothing left to do.
    #[test]
    fn a_write_on_a_message_gone_converges() {
        let gone = || {
            json(
                "404 Not Found",
                r#"{"error":{"code":404,"message":"Requested entity was not found.","status":"NOT_FOUND"}}"#,
            )
        };
        let (mut client, api, _) = scripted();

        api.answer(gone());
        client.delete_item("TRASH", "m1").unwrap();
        api.answer(gone());
        client.delete_item("INBOX", "m1").unwrap();
        api.answer(gone());
        client
            .store_flags(&["m1"], &[Flag::from_iana(IanaFlag::Flagged)], FlagOp::Add)
            .unwrap();

        // NOTE: a batch modify answered 404 is done again one by one.
        api.answer(gone());
        api.answer(ok(
            serde_json::json!({ "id": "m2", "labelIds": ["Label_1"] }),
        ));
        api.answer(gone());
        client.move_items("INBOX", "Work", &["m2", "m3"]).unwrap();
        let requests = api.requests();
        assert!(requests[3].contains("batchModify"), "{requests:?}");
        assert!(requests[4].contains("/messages/m2/modify"), "{requests:?}");
        assert!(requests[5].contains("/messages/m3/modify"), "{requests:?}");

        api.answer(json(
            "403 Forbidden",
            r#"{"error":{"code":403,"message":"Insufficient Permission"}}"#,
        ));
        assert!(
            client.delete_item("INBOX", "m1").is_err(),
            "any other refusal fails"
        );
    }
}
