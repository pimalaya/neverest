//! # Gmail client
//!
//! [`GmailClient`] wraps io-gmail's std client behind the adapter surface the
//! sync engine needs, syncing `message/rfc822` over the Gmail REST API.
//!
//! A message carries a set of labels, pimdir's one item in several collections:
//! the user labels plus [`SYSTEM_COLLECTIONS`], named as IMAP shows them, so
//! Gmail meets IMAP on one account. `UNREAD`, `STARRED` and `IMPORTANT` are
//! flags; the categories, partitioning `INBOX`, are left out.
//!
//! A handle is the Gmail message id, stable across labels, so no rekey ever
//! happens. The checkpoint is the mailbox `historyId`: a full round lists the
//! label's ids (plus three id-only flag listings), a delta projects the mailbox
//! history on the label, and an expired history id (404) restarts.
//!
//! A run resuming an open round lists its delta head first (pimdir SYNC §4),
//! the same projection from the round's history id, so arrivals show before it
//! closes; the label's later pages read what the head touched anew.
//!
//! Messages are read through Gmail's HTTP batch endpoint, a batch of a few
//! reads at a time across the source ([`super::batch`]), paced by the units its
//! reads cost; a message read under one label is not read again under another
//! in the same run ([`super::run`]).
//!
//! Pushes are label changes: a delete removes the collection's label (from
//! `INBOX` it archives), a move swaps two, and only a delete from `TRASH` is
//! permanent. A message gone counts as done. An append goes through
//! `messages.import`, its answered id checked before it is trusted.

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
            import::GmailMessageImport, list::GmailMessagesListParams, send::GmailMessageSend,
        },
    },
    send::{GMAIL_API_BASE, GmailApiError, GmailSendError, GmailSendOutput},
};
use io_pimdir::{
    collection::{PimdirCheckpoint, PimdirScope},
    remote::{PimdirEnumerate, PimdirHead, PimdirListing},
    summary::{
        PimdirDerivation,
        mail::{derive_meta, message_id},
    },
};
use log::{debug, trace, warn};
use pimalaya_cli::error::CodedError;
use pimalaya_stream::{proxy::Proxy, tls::Tls};
use secrecy::{ExposeSecret, SecretString};

use super::{
    batch::{self, Batcher, Read as ReadAs, Sizes},
    run::{GmailRun, History},
};
use crate::{
    account::GmailAccount,
    client::{
        EnumEntry, Enumeration, Gone, Held, Listed, Renamed, StoredSize, WrittenItem, floor_of,
    },
    collection::Collection,
    error::{ErrorCode, names_the_request},
    flag::{Flag, FlagOp, IanaFlag},
    http::open_tls,
    sync::invitation::Refusal,
    throttle::{
        GoogleAnswer, GoogleError, GoogleSession, Pick, Reopen, Request, Throttle, google_code,
        google_send, rfc3339,
    },
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

/// The system labels Gmail refuses in any label change, added or removed.
///
/// A message enters them by import alone and leaves them for the trash alone.
const FIXED_LABELS: &[&str] = &["DRAFT", "SENT"];

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

/// How far below a scope the `after:` search reaches: Gmail files a
/// message by its reception, which may differ from its `Date`.
const SCOPE_MARGIN_DAYS: i64 = 2;

/// The most ids one `batchModify` takes.
const BATCH_SIZE: usize = 1000;

/// How many times an import's history is read before giving up on it.
const IMPORT_ATTEMPTS: u32 = 3;

/// The quota units a second the Gmail source holds itself to, across all its
/// connections.
///
/// Gmail meters a user at 250 units a second, a `messages.get` costing 5,
/// batched or not: 200 is 40 reads a second, a margin below the quota. The
/// minute's budget ([`super::run::UNITS_PER_MINUTE`]) binds.
pub const UNITS_PER_SECOND: f64 = 200.0;

/// What one `messages.get` costs, a batch costing it for each read.
pub const MESSAGE_GET_UNITS: u32 = units::MESSAGE_GET;

/// What each Gmail call costs in quota units, as Gmail bills them.
mod units {
    pub const LABEL_GET: u32 = 1;
    pub const LABELS_LIST: u32 = 1;
    pub const LABEL_CREATE: u32 = 5;
    pub const LABEL_DELETE: u32 = 5;
    pub const LABEL_PATCH: u32 = 5;
    pub const PROFILE_GET: u32 = 1;
    pub const MESSAGES_LIST: u32 = 5;
    pub const HISTORY_LIST: u32 = 2;
    pub const MESSAGE_GET: u32 = 5;
    pub const MESSAGE_IMPORT: u32 = 25;
    pub const MESSAGE_SEND: u32 = 100;
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
    /// The session to the API, reopening its stream as it dies.
    inner: GoogleSession<GmailClientStd>,
    /// The session to the batch endpoint, opened at the first batch.
    batch: Option<GoogleSession<GmailClientStd>>,
    /// The bearer token, kept to open the batch session.
    token: SecretString,
    /// The TLS configuration, kept to open the batch session.
    tls: Tls,
    /// The proxy, kept to open the batch session.
    proxy: Proxy,
    /// Label name to label id, refreshed from the label listing when a name
    /// misses.
    labels: HashMap<String, String>,
    /// The source's pacing and back-off, shared by its connections.
    throttle: Arc<Throttle>,
    /// What the source's connections share for the run.
    run: Arc<GmailRun>,
    /// The flag sets of the last label and query a round page read, and when.
    ///
    /// Kept for the run so the next page does not list them again for nothing.
    flag_sets: Option<((String, Option<String>), Instant, FlagSets)>,
}

impl GmailClient {
    /// Opens the TLS connection to the Gmail API with a bearer token, scoped
    /// to the mailbox owner (`me` or an address).
    pub fn connect(account: &GmailAccount, throttle: Arc<Throttle>) -> Result<Self> {
        let options = GmailClientStdConnectOptions {
            tls: account.tls.clone(),
            proxy: account.proxy.clone(),
            user_id: account.user_id.clone(),
        };
        let inner = GmailClientStd::connect(account.token.expose_secret(), options)
            .context("Cannot connect to Gmail")?;
        let inner = GoogleSession::new(
            "gmail",
            inner,
            reopen(&account.tls, &account.proxy, GMAIL_API_BASE),
        );

        Ok(Self {
            inner,
            batch: None,
            token: account.token.clone(),
            tls: account.tls.clone(),
            proxy: account.proxy.clone(),
            labels: HashMap::new(),
            throttle,
            run: Arc::clone(&account.run),
            flag_sets: None,
        })
    }

    /// The session to the API.
    fn api_session(&mut self) -> Result<&mut GoogleSession<GmailClientStd>, GmailClientStdError> {
        Ok(&mut self.inner)
    }

    /// The session to the batch endpoint, opened or reopened when the
    /// server closed it.
    fn batch_session(&mut self) -> Result<&mut GoogleSession<GmailClientStd>, GmailClientStdError> {
        let broken =
            |err: anyhow::Error| GmailClientStdError::Io(io::Error::other(format!("{err:#}")));
        match &mut self.batch {
            Some(session) if !session.alive() => session.reopen().map_err(broken)?,
            Some(_) => {}
            None => {
                debug!("opening the gmail batch stream");
                let stream = open_tls(GMAIL_BATCH_URL, &self.tls, &self.proxy).map_err(broken)?;
                let options = GmailClientStdConnectOptions {
                    tls: self.tls.clone(),
                    proxy: self.proxy.clone(),
                    user_id: self.inner.client.user_id.clone(),
                };
                let token = self.token.expose_secret();
                let client = GmailClientStd::new(stream, token, options);
                let reopen = reopen(&self.tls, &self.proxy, GMAIL_BATCH_URL);
                self.batch = Some(GoogleSession::new("gmail batch", client, reopen));
            }
        }
        self.batch
            .as_mut()
            .ok_or_else(|| GmailClientStdError::Io(io::Error::other("no gmail batch session")))
    }

    /// Runs one Gmail operation costing `units` of quota, reopening a closed
    /// stream first and recording the new keep-alive hint.
    ///
    /// The source's [`Throttle`] paces it and resends it while Gmail throttles
    /// it (429, 502 to 504, a 403 rate limit), by Google's reason
    /// ([`google_send`]); creations go through [`create`](Self::create).
    fn op<T>(
        &mut self,
        units: u32,
        run: impl FnMut(&mut GmailClientStd) -> Result<GmailSendOutput<T>, GmailClientStdError>,
    ) -> Result<T, GmailClientStdError> {
        self.send(Host::Api, Request::Idempotent, units, run)
    }

    /// Runs one Gmail request that creates something (a label, an import).
    ///
    /// Sent again on a refusal Gmail answers before doing anything (429, a rate
    /// limit), never on a 5xx, after which it may exist.
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
        let pick: Pick<Self, GmailClientStd, GmailClientStdError> = match host {
            Host::Api => Self::api_session,
            Host::Batch => Self::batch_session,
        };
        google_send(self, pick, &throttle, request, units, |client| {
            run(client).map(|out| (out.response, out.keep_alive))
        })
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
        let run = Arc::clone(&self.run);
        let mut batcher = LiveBatcher { client: self };
        batch::read_all(&mut batcher, read, ids, &run.batch_size, sizes, each)
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
                takes_tasks: None,
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
    /// answering the collection name it is listed under.
    ///
    /// An existing label of that name is answered as is (pimdir Annex B.2
    /// `collection-create`).
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

    /// Renames the label `label` to `name` under its parent, its nested labels
    /// following, answering what it is then (pimdir Annex B.2).
    ///
    /// Gmail nests by name, so nested labels go one by one, deepest first, a
    /// cut run resuming from the old names; names match ignoring case. Done
    /// already is success; a system label, a taken name or one gone is refused.
    pub fn rename_label(&mut self, label: &str, name: &str) -> Result<Renamed> {
        let refused = |why: String| Err(anyhow::Error::new(Refusal(why)));

        if is_system(label) {
            return refused(format!("Gmail system label {label} keeps its name"));
        }
        let parent = label.rsplit_once('/').map(|(parent, _)| parent);
        let target = child_label(parent, name)?.name;
        let renamed = Renamed {
            id: target.clone(),
            name: target.clone(),
            delimiter: Some('/'),
        };
        if target == label {
            return Ok(renamed);
        }

        self.list_labels()?;
        let held = |labels: &HashMap<String, String>, name: &str| lookup_label(labels, name);
        let under = format!("{label}/");
        let mut nested: Vec<(String, String)> = self
            .labels
            .iter()
            .filter(|(name, _)| name.starts_with(&under))
            .map(|(name, id)| (name.clone(), id.clone()))
            .collect();
        match (held(&self.labels, label), held(&self.labels, &target)) {
            (Some(held), Some(other)) if held != other => {
                return refused(format!("Gmail label {target} already exists"));
            }
            (None, None) if nested.is_empty() => {
                return refused(format!("No Gmail label {label} to rename"));
            }
            _ => {}
        }

        nested.sort_by_key(|(name, _)| std::cmp::Reverse(name.matches('/').count()));
        if let Some(id) = held(&self.labels, label) {
            nested.push((label.to_owned(), id));
        }
        for (old, id) in nested {
            let new = format!("{target}{}", &old[label.len()..]);
            let patch = GmailLabel {
                id,
                name: new.clone(),
                ..Default::default()
            };
            self.op(units::LABEL_PATCH, |client| client.label_patch(&patch))
                .with_context(|| format!("Rename label {old} to {new} error"))?;
            self.labels.remove(&old);
            self.labels.insert(new, patch.id);
        }

        Ok(renamed)
    }

    /// Deletes the label `label` (pimdir Annex B.2 `collection-delete`).
    ///
    /// Refused with a label nested under it or as a system label; one already
    /// gone is success. Its messages keep their other labels, All Mail at
    /// least: Gmail deletes no labelled message.
    pub fn delete_label(&mut self, label: &str) -> Result<()> {
        if is_system(label) {
            return Err(anyhow::Error::new(Refusal(format!(
                "Gmail system label {label} cannot be deleted"
            ))));
        }
        self.list_labels()?;
        let Some(id) = lookup_label(&self.labels, label) else {
            return Ok(());
        };
        let under = format!("{label}/");
        let mut nested: Vec<&str> = self
            .labels
            .keys()
            .filter(|name| name.starts_with(&under))
            .map(String::as_str)
            .collect();
        if !nested.is_empty() {
            nested.sort();
            return Err(anyhow::Error::new(Refusal(format!(
                "Gmail label {label} holds {}: delete them first",
                nested.join(", ")
            ))));
        }

        self.op(units::LABEL_DELETE, |client| client.label_delete(&id))
            .with_context(|| format!("Delete label {label} error"))?;
        self.labels.retain(|_, held| *held != id);
        Ok(())
    }

    /// Lists one page of a label (pimdir SYNC §4), no band head: Gmail dates
    /// no label change.
    ///
    /// A delta projects the history since the checkpoint, an expired id
    /// rejecting it; a round lists newest first, [`ROUND_PAGE_SIZE`] ids a page
    /// under `after:` ([`floor_of`]).
    pub fn enumerate(
        &mut self,
        collection: &str,
        request: &PimdirEnumerate,
        held: Held<'_>,
    ) -> Result<Listed> {
        let label = self.label_id(collection)?;

        match &request.listing {
            PimdirListing::Head(PimdirHead::Delta(checkpoint)) => {
                self.delta_head(collection, &label, checkpoint, held)
            }
            PimdirListing::Head(PimdirHead::Band { .. }) => Ok(Listed::NoHead),
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
                let scope = floor_of(&request.scope);
                self.round_page(collection, &label, token, *band, &scope, held)
            }
        }
    }

    /// The head of an open round (pimdir SYNC §4): the label's changes since
    /// `checkpoint` as a delta, with the history id Gmail holds now.
    ///
    /// What it touched is recorded, so later pages read it anew.
    fn delta_head(
        &mut self,
        collection: &str,
        label: &str,
        checkpoint: &PimdirCheckpoint,
        held: Held<'_>,
    ) -> Result<Listed> {
        let at = Instant::now();
        let Some(start) = decode_checkpoint(&checkpoint.0) else {
            warn!("gmail head of {collection} asked from no history id, restarting the round");
            return Ok(Listed::HeadRejected);
        };
        let changes = match self.changes(label, &start, &held) {
            Ok(changes) => changes,
            Err(err) if is_not_found(&err) => {
                warn!("gmail history of {collection} expired, restarting the round");
                return Ok(Listed::HeadRejected);
            }
            Err(err) => {
                return Err(
                    anyhow::Error::new(err).context(format!("List the head of {collection} error"))
                );
            }
        };

        let touched: Vec<String> = changes
            .changed
            .iter()
            .chain(&changes.deleted)
            .cloned()
            .collect();
        let head = self.delta_round(collection, label, changes, held)?;
        self.run.headed(label, at, touched);
        Ok(Listed::Page(head))
    }

    /// One page of a round over `scope`, from its start or from `token`.
    ///
    /// Bound and kept members are flagged from this label's listings, the rest
    /// read; one a head touched is read after the head, one gone left out, and
    /// a narrowed last page hands all the label's ids.
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
        let head = run.head(label);
        let since = head.as_ref().map(|head| head.at);
        let touched = |id: &str| head.as_ref().is_some_and(|head| head.touched.contains(id));
        let mut bound = Vec::new();
        let mut kept = Vec::new();
        let mut fresh = Vec::new();
        let mut labels_only = Vec::new();
        let mut unread = Vec::new();
        for message in page.messages {
            let id = message.id;
            let holds = held.holds(&id);
            if !touched(&id) {
                if holds {
                    bound.push(id);
                } else if let Some(message) = run.seen(&id, true, None) {
                    kept.push((id, message));
                } else {
                    unread.push(id);
                }
            } else if let Some(message) = run.seen(&id, !holds, since) {
                fresh.push((id, holds, message));
            } else if holds {
                labels_only.push(id);
            } else {
                unread.push(id);
            }
        }

        let mut items = Vec::with_capacity(bound.len() + kept.len() + unread.len());
        if !bound.is_empty() || !kept.is_empty() {
            let flags = self.flag_sets(label, query.as_deref(), since)?;
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

        let mut left = Vec::new();
        for (id, holds, message) in fresh {
            settle(label, id, holds, &message, &mut items, &mut left);
        }
        let shared = self.collection_ids();
        let mut bytes = 0;
        for (read, ids) in [(ReadAs::Minimal, labels_only), (ReadAs::Metadata, unread)] {
            let at = Instant::now();
            let holds = read == ReadAs::Minimal;
            self.read_messages(read, &ids, &|_| 0, |id, message| {
                let Some(message) = message else {
                    return Ok(());
                };
                if !holds {
                    bytes += serde_json::to_vec(&message).map_or(0, |json| json.len() as u64);
                }
                keep(&run, &shared, &id, &message, !holds, at);
                settle(label, id, holds, &message, &mut items, &mut left);
                Ok(())
            })
            .with_context(|| format!("Read the metadata of {collection} error"))?;
        }
        if !left.is_empty() {
            debug!(
                "{} member(s) of {collection} left the label since it was listed",
                left.len()
            );
            trace!("left: {left:?}");
        }

        let narrowed = query.is_some();
        let enumerated = match narrowed && page.next_page_token.is_none() {
            true => Some(
                self.list_ids(label, None, None)
                    .with_context(|| format!("Enumerate the ids of {collection} error"))?,
            ),
            false => None,
        };

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
            narrowed,
            enumerated,
            truncated: false,
            errored: Vec::new(),
        }))
    }

    /// The flag-like labels of a label's messages under `query`.
    ///
    /// Three id-only listings read once a run rather than one get a message,
    /// and again when read before `since`, the label's head.
    fn flag_sets(
        &mut self,
        label: &str,
        query: Option<&str>,
        since: Option<Instant>,
    ) -> Result<FlagSets> {
        let key = (label.to_owned(), query.map(String::from));
        if let Some((cached, at, sets)) = &self.flag_sets
            && *cached == key
            && since.is_none_or(|since| *at >= since)
        {
            return Ok(sets.clone());
        }

        let at = Instant::now();
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
        self.flag_sets = Some((key, at, sets.clone()));
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

    /// Reads the history since `start`, paged: the mailbox's whole, or
    /// `label`'s.
    ///
    /// Answers the records and the history id the next round starts from; the
    /// error comes back unwrapped, telling an expired history id (404) apart.
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
    /// Records are increments, not state, so each changed message is read again
    /// (its labels, or its metadata when unbound): still labelled, it is an
    /// item; otherwise or gone, it vanished.
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

    /// What names each of an id set, by batches of metadata reads; a
    /// message gone is left out.
    pub fn fetch_summaries(&mut self, ids: &[&str]) -> Result<Vec<(String, PimdirDerivation)>> {
        let ids: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let run = Arc::clone(&self.run);
        let shared = self.collection_ids();
        let at = Instant::now();
        let mut summaries = Vec::with_capacity(ids.len());
        self.read_messages(ReadAs::Metadata, &ids, &|_| 0, |id, message| {
            if let Some(message) = message {
                keep(&run, &shared, &id, &message, true, at);
                let meta = message_meta(&message);
                summaries.push((id, meta));
            }
            Ok(())
        })
        .context("Read message metadata error")?;
        Ok(summaries)
    }

    /// Streams the bodies of an id set by batches of raw reads, each written to
    /// its sink as its batch lands, answering the ids gone since listed.
    ///
    /// Sizes come from the run's metadata reads, else the store (`stored`),
    /// else a minimal read; a batch holds about [`batch::RAW_BATCH_BYTES`], one
    /// past [`batch::ALONE_BYTES`] alone.
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

    /// Gives the API session a new connection, so a send never goes out
    /// over one Gmail closed while it idled.
    pub fn fresh_stream(&mut self) -> Result<()> {
        self.inner.reopen()
    }

    /// Sends raw MIME through `messages.send`, which files the message in
    /// SENT itself.
    ///
    /// Sent again on a refusal alone ([`create`](Self::create)). Gmail reads
    /// the recipients from the headers, `Bcc` included and stripped: other
    /// envelope ones are lost.
    pub fn send_raw(&mut self, raw: &[u8]) -> Result<(), GmailClientStdError> {
        let message = GmailMessage {
            raw: Some(encode_raw(raw)),
            ..Default::default()
        };
        self.create(units::MESSAGE_SEND, |client| {
            let coroutine = GmailMessageSend::new(&client.auth, &client.user_id, &message)?;
            client.run(coroutine)
        })
        .map(|_| ())
    }

    /// Imports a message into a label with its flags, keeping its own date
    /// and `Message-ID`, and returns the id it really got.
    ///
    /// `import` may answer an id that 404s, so the answer is checked; on a miss
    /// the message is found by `hint`, a derived `Message-ID` compared as it
    /// stands, among what the label's history added since the call.
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

        let Some(hint) = hint.filter(|hint| !hint.is_empty()) else {
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
                let stated = message
                    .payload
                    .as_ref()
                    .and_then(|payload| payload.header("Message-ID"))
                    .and_then(message_id);
                if stated.as_deref() == Some(hint) {
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

    /// Deletes one message from a label by removing that label.
    ///
    /// From `INBOX` it archives, from `TRASH` it deletes for good, from `DRAFT`
    /// and `SENT` it trashes; a message gone already is deleted.
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
        } else if FIXED_LABELS.contains(&label.as_str()) {
            self.modify(&[id], &[TRASH.to_owned()], &[])
                .with_context(|| format!("Trash message {id} of {collection} error"))?;
        } else {
            self.modify(&[id], &[], &[label])
                .with_context(|| format!("Remove {collection} from message {id} error"))?;
        }

        Ok(())
    }

    /// Moves messages from one label to another in one modify each batch.
    ///
    /// Out of `DRAFT` or `SENT` only to `TRASH`, which trashes them, and never
    /// into them: Gmail refuses either label in a change, coded `unsupported`.
    pub fn move_items(&mut self, from: &str, to: &str, ids: &[&str]) -> Result<()> {
        let from_label = self.label_id(from)?;
        let to_label = self.label_id(to)?;
        if from_label == to_label {
            return Ok(());
        }
        if FIXED_LABELS.contains(&from_label.as_str()) && to_label == TRASH {
            return self
                .modify(ids, &[to_label], &[])
                .with_context(|| format!("Trash messages of {from} error"));
        }
        if let Some(fixed) = [&from_label, &to_label]
            .into_iter()
            .find(|label| FIXED_LABELS.contains(&label.as_str()))
        {
            let message = format!(
                "Gmail moves no message into or out of {fixed}, only imports into it \
                 or trashes from it"
            );
            return Err(CodedError::new(ErrorCode::Unsupported, message).into());
        }

        self.modify(ids, &[to_label], &[from_label])
            .with_context(|| format!("Move messages from {from} to {to} error"))
    }

    /// Adds `add` and removes `remove` as `UNREAD`, `STARRED` and `IMPORTANT`
    /// label changes.
    ///
    /// One modify names only the labels they map to; flags with no label drop.
    pub fn update_flags(&mut self, ids: &[&str], add: &[Flag], remove: &[Flag]) -> Result<()> {
        let (mut labels_add, mut labels_remove) = label_patch(add, FlagOp::Add);
        let (more_add, more_remove) = label_patch(remove, FlagOp::Remove);
        labels_add.extend(more_add);
        labels_remove.extend(more_remove);
        if labels_add.is_empty() && labels_remove.is_empty() {
            return Ok(());
        }

        self.modify(ids, &labels_add, &labels_remove)
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
    /// One batch to the batch endpoint, through the source's gate (one batch in
    /// flight by default) and paced by the units of all its reads.
    ///
    /// Each read is tagged by its index, whatever its id holds.
    fn batch(
        &mut self,
        ids: &[String],
        read: ReadAs,
    ) -> Result<Vec<GmailBatchResponse<GmailMessage>>> {
        let user = self.client.inner.client.user_id.clone();
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
            let message = format!(
                "Gmail gave up after being throttled, until {}",
                rfc3339(until)
            );
            return Err(CodedError::new(ErrorCode::RateLimited, message).into());
        }
        if !throttle.waits_in_process(wait) {
            let until = SystemTime::now() + wait;
            throttle.give_up(until);
            let message = format!("Gmail throttled, giving up until {}", rfc3339(until));
            return Err(CodedError::new(ErrorCode::RateLimited, message).into());
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

impl GoogleError for GmailClientStdError {
    fn broke(&self) -> bool {
        matches!(self, Self::Io(_))
    }

    fn answer(&self) -> Option<GoogleAnswer<'_>> {
        match self {
            Self::Send(GmailSendError::Api(err)) => Some(google_answer(err)),
            _ => None,
        }
    }

    fn throttled(message: String) -> Self {
        Self::Send(GmailSendError::Api(GmailApiError {
            status: 429,
            message,
            ..Default::default()
        }))
    }
}

/// Gives a Gmail client a new stream to the host of `url`, through `proxy`.
fn reopen(tls: &Tls, proxy: &Proxy, url: &'static str) -> Reopen<GmailClientStd> {
    let (tls, proxy) = (tls.clone(), proxy.clone());
    Box::new(move |gmail| {
        gmail.set_stream(open_tls(url, &tls, &proxy)?);
        Ok(())
    })
}

/// Whether a Gmail answer in an error chain refused the change as it stands.
///
/// Its status names the request ([`names_the_request`]); a rate limit is told
/// apart by [`crate::error::refusal`].
pub(crate) fn refuses(cause: &(dyn std::error::Error + 'static)) -> bool {
    match cause.downcast_ref::<GmailClientStdError>() {
        Some(GmailClientStdError::Send(GmailSendError::Api(err))) => names_the_request(err.status),
        _ => false,
    }
}

/// The code of a Gmail refusal in an error chain ([`google_code`]).
pub(crate) fn refusal_code(cause: &(dyn std::error::Error + 'static)) -> Option<ErrorCode> {
    match cause.downcast_ref::<GmailClientStdError>()? {
        GmailClientStdError::Send(GmailSendError::Api(err)) => google_code(&google_answer(err)),
        _ => None,
    }
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

/// Keeps a message read at `at` for the rest of the run.
///
/// Whole when it carries another label synced as a collection, which will list
/// it too, else its size alone, for its body read.
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
        meta: Some(message_meta(message)),
    }
}

/// The search narrowing a listing to a scope's floor.
///
/// `after:` [`SCOPE_MARGIN_DAYS`] below `since`, in epoch seconds read as an
/// instant, never `before:`, which reads the arrival ([`floor_of`]): a superset
/// of the scope by `Date`.
fn scope_query(scope: &PimdirScope) -> Option<String> {
    let since = DateTime::parse_from_rfc3339(scope.since.as_deref()?).ok()?;
    let at = since.checked_sub_signed(chrono::Duration::days(SCOPE_MARGIN_DAYS))?;
    Some(format!("after:{}", at.timestamp()))
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

/// The label a `collection-create` asks Gmail for: `name` under `parent`.
///
/// Gmail nests a label by its name (`Parent/Child`); a system label takes no
/// child, and a name holding `/` would nest by itself.
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

/// Whether a message carrying `label_ids` is a member of `label`'s collection.
///
/// A spam or trashed message is one of `SPAM` or `TRASH` only, as the listings
/// exclude it everywhere else.
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

/// What names a message read as metadata (pimdir STORAGE Annex A.1).
///
/// The headers a summary reads are laid out as their header block, each from
/// its first entry, unfolded, and read by io-pimdir's body derivation, so both
/// tiers agree. Size and attachment mark are Gmail's, restated at `Full`.
fn message_meta(message: &GmailMessage) -> PimdirDerivation {
    let mut header = String::new();
    for name in batch::SUMMARY_HEADERS {
        let stated = message.payload.as_ref().and_then(|p| p.header(name));
        if let Some(value) = stated {
            header.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    header.push_str("\r\n");

    let size = message.size_estimate.filter(|size| *size > 0);
    derive_meta(header.as_bytes(), size, None)
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
    use super::*;
    use crate::scripted::Server;

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
    pub(crate) fn scripted() -> (GmailClient, Server, Server) {
        let api = Server::new(vec![Vec::new()]);
        let batch = Server::new(vec![Vec::new()]);
        let client = GmailClient {
            inner: session("gmail", &api),
            batch: Some(session("gmail batch", &batch)),
            token: SecretString::from(String::from("token")),
            tls: Tls::default(),
            proxy: Proxy::None,
            labels: HashMap::from([
                (String::from("INBOX"), String::from("INBOX")),
                (String::from("Work"), String::from("Label_1")),
                (String::from("TRASH"), String::from("TRASH")),
                (String::from("DRAFT"), String::from("DRAFT")),
                (String::from("SENT"), String::from("SENT")),
            ]),
            throttle: Arc::new(Throttle::new("gmail")),
            run: Arc::new(GmailRun::new(Default::default())),
            flag_sets: None,
        };
        (client, api, batch)
    }

    /// A Gmail session named `name` over the connections `server` opens.
    pub(crate) fn session(name: &'static str, server: &Server) -> GoogleSession<GmailClientStd> {
        let options = GmailClientStdConnectOptions {
            tls: Tls::default(),
            proxy: Proxy::None,
            user_id: String::from("me"),
        };
        let client = GmailClientStd::new(server.open().unwrap(), "token", options);
        let opener = server.clone();
        let reopen: Reopen<GmailClientStd> = Box::new(move |gmail| {
            gmail.set_stream(opener.open()?);
            Ok(())
        });
        GoogleSession::new(name, client, reopen)
    }
}

#[cfg(test)]
mod tests {
    use io_gmail::v1::send::{GmailApiError, GmailSendError};
    use io_pimdir::summary::PimdirSummary;

    use super::scripted::*;
    use super::*;
    use crate::{
        error::Failure,
        kind::mail::vectors::VECTORS,
        scripted::{Server, json, ok},
    };

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

        let entry = named_entry("18c", &message);
        let Some(PimdirSummary::Mail(summary)) = entry.meta.and_then(|meta| meta.summary) else {
            panic!("expected a mail summary");
        };
        assert_eq!(summary.message_id.as_deref(), Some("m1@example.org"));
        assert_eq!(summary.in_reply_to, ["m0@example.org"]);
        assert_eq!(summary.subject, "Café");
        assert_eq!(summary.from[0].name.as_deref(), Some("Alice"));
        assert_eq!(summary.from[0].address, "alice@example.org");
        assert_eq!(summary.to.len(), 2);
        assert_eq!(summary.date.as_deref(), Some("2026-07-06T12:00:00Z"));
        assert!(entry.flags.is_empty());
        assert_eq!(summary.size, Some(2048));
    }

    /// A message is named off its metadata headers as its body would name it
    /// (pimdir STORAGE Annex A.1, vectors/summaries.json).
    ///
    /// A commented `Message-ID`, a folded `Subject` and a field stated twice
    /// among them.
    #[test]
    fn a_metadata_get_is_keyed_as_the_vectors_state() {
        for vector in &VECTORS {
            let headers: Vec<serde_json::Value> = vector
                .fields()
                .into_iter()
                .map(|(name, value)| serde_json::json!({ "name": name, "value": value }))
                .collect();
            let message: GmailMessage = serde_json::from_value(serde_json::json!({
                "id": "18c",
                "labelIds": ["INBOX"],
                "sizeEstimate": vector.body.len(),
                "payload": { "headers": headers },
            }))
            .unwrap();

            let entry = named_entry("18c", &message);
            vector.assert_named(&entry.meta.unwrap());
        }
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

    /// A listed message is named by its metadata.
    ///
    /// The `Date` header, never Gmail's `internalDate`, the flags of its
    /// labels, and the attachment mark from the top-level `Content-Type`, both
    /// ways.
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

    /// The search reaches two days below the scope and never caps a band.
    ///
    /// Epoch seconds are instants, Gmail's own midnights being Pacific;
    /// `before:` reads the arrival, which may lie long after the `Date`.
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
        assert_eq!(scope_query(&band).as_deref(), Some("after:1790812800"));
        let below = PimdirScope {
            since: None,
            until: Some(String::from("2026-10-05T00:00:00Z")),
        };
        assert_eq!(scope_query(&below), None);
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
            Listed::CursorRejected | Listed::NoHead | Listed::HeadRejected | Listed::Rekey => {
                panic!("a page")
            }
        }
    }

    /// A message under two labels is read once a run.
    ///
    /// The second label's listing names it from what the first read, its flags
    /// from its own flag listings (another client read it since).
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
        api.answer(listed(&[]));
        api.answer(listed(&["m1"]));
        api.answer(listed(&[]));
        let work = page(client.enumerate("Work", &round(), held(&none)).unwrap());

        assert_eq!(batch.lines().len(), 1, "one batch in all");
        assert!(batch.lines()[0].starts_with("POST /batch/gmail/v1"));
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
            .lines()
            .into_iter()
            .filter(|line| line.contains("/history"))
            .collect();
        assert_eq!(history.len(), 1, "{history:?}");
        assert!(!history[0].contains("labelId"), "{}", history[0]);
        assert_eq!(batch.lines().len(), 2, "m1 read once, m3 once");
        assert_eq!(api.left() + batch.left(), 0);
    }

    /// A band round narrowed by `after:` hands every id of the label on its
    /// last page, as a scoped round does (pimdir SYNC §4, §5).
    ///
    /// A narrowed round handing none finds no member absent, so a delete in the
    /// band would go unseen (pimdir vectors 160, 161).
    #[test]
    fn a_band_round_hands_the_label_s_ids_on_its_last_page() {
        let (mut client, api, _) = scripted();
        let bound = HashSet::from([String::from("m1")]);
        let band = PimdirEnumerate {
            listing: PimdirListing::Round {
                cursor: None,
                band: true,
            },
            scope: PimdirScope {
                since: Some(String::from("2026-01-01T00:00:00Z")),
                until: Some(String::from("2026-10-16T00:00:00Z")),
            },
        };

        api.answer(listed(&["m1"]));
        api.answer(listed(&[]));
        api.answer(listed(&[]));
        api.answer(listed(&[]));
        api.answer(listed(&["m1", "m9"]));
        let last = page(client.enumerate("INBOX", &band, held(&bound)).unwrap());

        assert!(last.narrowed);
        let mut enumerated = last.enumerated.expect("the label's ids");
        enumerated.sort();
        assert_eq!(enumerated, ["m1", "m9"]);
        let requests = api.lines();
        let unfiltered = requests.last().expect("the enumeration");
        assert!(!unfiltered.contains("q="), "{requests:?}");
        assert_eq!(api.left(), 0, "{requests:?}");
    }

    /// A delete Gmail refuses for rights is coded `permission-denied`, a
    /// refused request left uncoded.
    #[test]
    fn a_refused_delete_is_coded_by_its_status() {
        let denied = r#"{"error":{"code":403,"message":"Insufficient Permission","errors":[{"reason":"insufficientPermissions"}]}}"#;
        let invalid = r#"{"error":{"code":400,"message":"Invalid id value","errors":[{"reason":"invalidArgument"}]}}"#;
        let cases = [
            ("403 Forbidden", denied, Some(ErrorCode::PermissionDenied)),
            ("400 Bad Request", invalid, None),
        ];

        for (status, body, code) in cases {
            let (mut client, api, _) = scripted();
            api.answer(json(status, body));
            let err = client.delete_item("TRASH", "m1").unwrap_err();

            let failure = Failure::of(&err);
            assert_eq!(failure.code, code, "{status}: {err:#}");
            assert_eq!(failure.error, "Delete message m1 error", "{err:#}");
        }
    }

    /// A write Gmail answers with a status naming the request is refused.
    ///
    /// A 400 or a 403 for rights is refused as it stands; a stale base (409,
    /// 412) and a rate limit (a 403 naming one) stay rejected (pimdir SYNC
    /// §4).
    #[test]
    fn a_write_refused_as_it_stands_is_told_by_its_status() {
        let invalid = r#"{"error":{"code":400,"message":"Invalid id value","errors":[{"reason":"invalidArgument"}]}}"#;
        let denied = r#"{"error":{"code":403,"message":"Insufficient Permission","errors":[{"reason":"insufficientPermissions"}]}}"#;
        let conflict =
            r#"{"error":{"code":409,"message":"Conflict","errors":[{"reason":"conflict"}]}}"#;
        let stale = r#"{"error":{"code":412,"message":"Precondition Failed","errors":[{"reason":"conditionNotMet"}]}}"#;
        let daily = r#"{"error":{"code":403,"message":"Daily Limit Exceeded","errors":[{"reason":"dailyLimitExceeded"}]}}"#;
        let cases = [
            ("400 Bad Request", invalid, true),
            ("403 Forbidden", denied, true),
            ("409 Conflict", conflict, false),
            ("412 Precondition Failed", stale, false),
            ("403 Forbidden", daily, false),
        ];

        for (status, body, refused) in cases {
            let (mut client, api, _) = scripted();
            api.answer(json(status, body));
            let err = client.delete_item("TRASH", "m1").unwrap_err();

            let reason = crate::error::refusal(&err);
            assert_eq!(reason.is_some(), refused, "{status} {body}: {err:#}");
        }
    }

    /// A send posts the raw message to `messages.send` over a new connection,
    /// and a refusal comes back with its status readable.
    #[test]
    fn a_send_posts_the_raw_message_over_a_fresh_stream() {
        let raw = b"To: b@y.org\r\nBcc: c@y.org\r\nSubject: hi\r\n\r\nhello";
        let (mut client, _, _) = scripted();
        let api = Server::new(vec![Vec::new(), Vec::new()]);
        client.inner = session("gmail", &api);

        client.fresh_stream().unwrap();
        api.answer(json("200 OK", r#"{"id":"m9","threadId":"t9"}"#));
        client.send_raw(raw).unwrap();

        assert_eq!(api.requests().len(), 2, "a second connection");
        assert_eq!(api.requests()[1], ["POST /gmail/v1/users/me/messages/send"]);
        let sent = api.written(1);
        assert!(sent.contains(&encode_raw(raw)), "{sent}");

        let invalid = r#"{"error":{"code":400,"message":"Recipient address required","errors":[{"reason":"invalidArgument"}]}}"#;
        api.answer(json("400 Bad Request", invalid));
        let err = client.send_raw(raw).unwrap_err();
        assert!(
            matches!(&err, GmailClientStdError::Send(GmailSendError::Api(api)) if api.status == 400),
            "{err:#}"
        );
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

    fn head(from: &str) -> PimdirEnumerate {
        PimdirEnumerate {
            listing: PimdirListing::Head(PimdirHead::Delta(PimdirCheckpoint(
                from.as_bytes().to_vec(),
            ))),
            scope: PimdirScope::unbounded(),
        }
    }

    fn resumed(token: &str) -> PimdirEnumerate {
        PimdirEnumerate {
            listing: PimdirListing::Round {
                cursor: Some(io_pimdir::collection::PimdirCursor(
                    token.as_bytes().to_vec(),
                )),
                band: false,
            },
            scope: PimdirScope::unbounded(),
        }
    }

    fn ids(page: &Enumeration) -> Vec<&str> {
        let mut ids: Vec<&str> = page.items.iter().map(|item| item.id.as_str()).collect();
        ids.sort();
        ids
    }

    /// A delta head lists what changed in the label since the round's history
    /// id, as a delta does.
    ///
    /// An arrival named by its metadata, a deletion stated at once, and the
    /// history id Gmail holds now.
    #[test]
    fn a_head_lists_the_label_since_the_round_began() {
        let (mut client, api, batch) = scripted();
        let m1 = HashSet::from([String::from("m1")]);

        api.answer(ok(serde_json::json!({
            "history": [
                { "id": "110", "messagesAdded": [{
                    "message": { "id": "m9", "labelIds": ["INBOX", "UNREAD"] }
                }]},
                { "id": "111", "messagesDeleted": [{
                    "message": { "id": "m1", "labelIds": ["INBOX"] }
                }]},
                { "id": "112", "messagesAdded": [{
                    "message": { "id": "m8", "labelIds": ["Label_1"] }
                }]},
            ],
            "historyId": "120"
        })));
        batch.answer(batch_answer(&[(
            0,
            "200 OK",
            metadata("m9", &["INBOX", "UNREAD"]),
        )]));
        let head = page(client.enumerate("INBOX", &head("100"), held(&m1)).unwrap());

        assert_eq!(ids(&head), ["m9"], "m8 is not the inbox's");
        assert!(head.items[0].meta.is_some(), "named by its metadata");
        assert_eq!(head.vanished, ["m1"]);
        assert_eq!(head.checkpoint.as_deref(), Some(&b"120"[..]));
        assert!(!head.complete, "a head is no round page");
        assert!(head.cursor.is_none(), "and moves no cursor");
        let history = api.lines();
        assert!(history[0].contains("/history"), "{history:?}");
        assert!(history[0].contains("startHistoryId=100"), "{history:?}");
        assert_eq!(api.left() + batch.left(), 0);
    }

    /// A head from a history id Gmail no longer holds is rejected, so the
    /// round restarts; Gmail lists no band head.
    #[test]
    fn an_expired_history_rejects_the_head() {
        let (mut client, api, _) = scripted();
        let none = HashSet::new();
        api.answer(json(
            "404 Not Found",
            r#"{"error":{"code":404,"message":"Requested entity was not found.","status":"NOT_FOUND"}}"#,
        ));
        let listed = client.enumerate("INBOX", &head("1"), held(&none)).unwrap();
        assert!(matches!(listed, Listed::HeadRejected), "{listed:?}");

        let listed = client.enumerate("INBOX", &head(""), held(&none)).unwrap();
        assert!(matches!(listed, Listed::HeadRejected), "no history id");

        let band = PimdirEnumerate {
            listing: PimdirListing::Head(PimdirHead::Band {
                since: String::from("2026-10-13T00:00:00Z"),
            }),
            scope: PimdirScope::unbounded(),
        };
        let listed = client.enumerate("INBOX", &band, held(&none)).unwrap();
        assert!(matches!(listed, Listed::NoHead), "{listed:?}");
        assert_eq!(api.lines().len(), 1, "only the expired history asked");
    }

    /// A page after a head never lists a member as it stood before the head.
    ///
    /// What it touched is read again unless read since (m3, read before it),
    /// its labels deciding; m1 and m4, off the label but still in the page
    /// token's list, are left out.
    #[test]
    fn a_page_after_a_head_reads_what_it_touched_anew() {
        let (mut client, api, batch) = scripted();
        let held_ids = HashSet::from([String::from("m1"), String::from("m2")]);

        let m3 = serde_json::from_value(metadata("m3", &["INBOX", "Label_1"])).unwrap();
        client.run.keep(&m3, true, Instant::now());

        api.answer(ok(serde_json::json!({
            "history": [
                { "id": "110", "labelsAdded": [{
                    "message": { "id": "m1", "labelIds": ["INBOX", "STARRED"] },
                    "labelIds": ["STARRED"]
                }]},
                { "id": "111", "labelsAdded": [{
                    "message": { "id": "m3", "labelIds": ["INBOX", "Label_1"] },
                    "labelIds": ["INBOX"]
                }]},
            ],
            "historyId": "120"
        })));
        batch.answer(batch_answer(&[(
            0,
            "200 OK",
            serde_json::json!({ "id": "m1", "labelIds": ["INBOX", "STARRED"] }),
        )]));
        batch.answer(batch_answer(&[(
            0,
            "200 OK",
            metadata("m3", &["INBOX", "Label_1"]),
        )]));
        let head = page(
            client
                .enumerate("INBOX", &head("100"), held(&held_ids))
                .unwrap(),
        );
        assert_eq!(ids(&head), ["m1", "m3"]);
        assert_eq!(
            batch.lines().len(),
            2,
            "m3 read again: the run's read predates the history"
        );

        api.answer(ok(serde_json::json!({
            "messages": [{ "id": "m1" }, { "id": "m2" }, { "id": "m3" }, { "id": "m4" }]
        })));
        api.answer(listed(&[]));
        api.answer(listed(&[]));
        api.answer(listed(&[]));
        batch.answer(batch_answer(&[(
            0,
            "200 OK",
            serde_json::json!({ "id": "m1", "labelIds": ["STARRED"] }),
        )]));
        batch.answer(batch_answer(&[(0, "200 OK", metadata("m4", &["Label_1"]))]));
        let page = page(
            client
                .enumerate("INBOX", &resumed("t2"), held(&held_ids))
                .unwrap(),
        );

        assert_eq!(
            ids(&page),
            ["m2", "m3"],
            "m1 and m4 left out, m3 as the head read it"
        );
        assert!(page.vanished.is_empty(), "a round page states no removal");
        let reads = batch.lines();
        assert_eq!(reads.len(), 4, "m1's labels and m4's metadata: {reads:?}");
        assert_eq!(api.left() + batch.left(), 0);
    }

    /// The flag sets a page read before the label's head are read again
    /// by the next page.
    #[test]
    fn flag_sets_read_before_a_head_are_read_again() {
        let (mut client, api, _) = scripted();
        let m1 = HashSet::from([String::from("m1")]);

        api.answer(listed(&["m1"]));
        api.answer(listed(&["m1"]));
        api.answer(listed(&[]));
        api.answer(listed(&[]));
        let first = page(
            client
                .enumerate("INBOX", &resumed("t1"), held(&m1))
                .unwrap(),
        );
        assert!(
            !first.items[0]
                .flags
                .contains(&Flag::from_iana(IanaFlag::Seen))
        );

        api.answer(ok(serde_json::json!({ "history": [], "historyId": "120" })));
        client.enumerate("INBOX", &head("100"), held(&m1)).unwrap();

        api.answer(listed(&["m1"]));
        api.answer(listed(&[]));
        api.answer(listed(&[]));
        api.answer(listed(&[]));
        let second = page(
            client
                .enumerate("INBOX", &resumed("t2"), held(&m1))
                .unwrap(),
        );
        assert!(
            second.items[0]
                .flags
                .contains(&Flag::from_iana(IanaFlag::Seen)),
            "read since the head"
        );
        assert_eq!(api.left(), 0);
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

        assert_eq!(batch.lines().len(), 2, "the sizes, then the bodies");
        assert_eq!(bodies.len(), 2, "m5 is gone");
        assert_eq!(gone, ["m5"], "and answered gone");
        assert!(bodies["m4"].ends_with("Body of m4\r\n"));
        assert_eq!(batch.left(), 0);
    }

    fn raw(id: &str) -> serde_json::Value {
        let source = format!("Message-ID: <{id}@example.org>\r\n\r\nBody of {id}\r\n");
        serde_json::json!({ "id": id, "raw": encode_raw(source.as_bytes()) })
    }

    /// The batch size lives on the run: a concurrency refusal a read meets cuts
    /// it there, and the next read of the source starts from it.
    ///
    /// From 2, a full batch of two doubles it to 4 and m5 alone leaves it; a
    /// read starting at the ceiling would send all three, miss m5 and end at 2.
    #[test]
    fn a_refusal_cuts_the_size_the_run_shares() {
        let (mut client, _, batch) = scripted();
        client.run.keep_size("m1", 100);
        client.run.keep_size("m2", 100);
        let concurrency = serde_json::json!({ "error": {
            "code": 429,
            "message": "Too many concurrent requests for user",
            "errors": [{ "domain": "global", "reason": "rateLimitExceeded" }],
            "status": "RESOURCE_EXHAUSTED",
        }});
        batch.answer(batch_answer(&[
            (0, "200 OK", raw("m1")),
            (1, "429 Too Many Requests", concurrency),
        ]));
        batch.answer(batch_answer(&[(0, "200 OK", raw("m2"))]));
        for id in ["m3", "m4", "m5"] {
            client.run.keep_size(id, 100);
        }
        batch.answer(batch_answer(&[
            (0, "200 OK", raw("m3")),
            (1, "200 OK", raw("m4")),
        ]));
        batch.answer(batch_answer(&[(0, "200 OK", raw("m5"))]));

        let mut bodies = Vec::new();
        client
            .fetch_bodies(
                &["m1", "m2"],
                &|_| None,
                |_| Ok(Vec::new()),
                |id, _, _| {
                    bodies.push(id.to_owned());
                    Ok(())
                },
            )
            .unwrap();

        assert_eq!(bodies.len(), 2);
        assert_eq!(batch.lines().len(), 2, "the refused body read again alone");
        assert_eq!(
            client.run.batch_size.lock().unwrap().current(),
            2,
            "cut to one by the refused batch of two, doubled by the full batch of one"
        );

        client
            .fetch_bodies(
                &["m3", "m4", "m5"],
                &|_| None,
                |_| Ok(Vec::new()),
                |id, _, _| {
                    bodies.push(id.to_owned());
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(bodies.len(), 5);
        assert_eq!(batch.lines().len(), 4);
        assert_eq!(client.run.batch_size.lock().unwrap().current(), 4);
        assert_eq!(batch.left(), 0);
    }

    /// A batched read refused past the waits is coded: `rate-limited` for a
    /// quota or a wait over the longest, `permission-denied` for rights.
    #[test]
    fn a_batched_read_refused_past_the_waits_is_coded() {
        let daily = serde_json::json!({ "error": {
            "code": 403,
            "message": "Quota exceeded for quota metric 'Queries' and limit 'Queries per day'",
            "errors": [{ "domain": "usageLimits", "reason": "dailyLimitExceeded" }],
        }});
        let minute = serde_json::json!({ "error": {
            "code": 403,
            "message": "Quota exceeded for quota metric 'Total Query Cost' and limit 'Units per minute per user'",
            "errors": [{ "domain": "usageLimits", "reason": "rateLimitExceeded" }],
        }});
        let denied = serde_json::json!({ "error": {
            "code": 403,
            "message": "Insufficient Permission",
            "errors": [{ "reason": "insufficientPermissions" }],
        }});
        let cases = [
            (daily, ErrorCode::RateLimited),
            (minute, ErrorCode::RateLimited),
            (denied, ErrorCode::PermissionDenied),
        ];

        for (body, code) in cases {
            let (mut client, _, batch) = scripted();
            let throttle = Throttle::new("gmail").waiting(Duration::from_secs(1));
            client.throttle = Arc::new(throttle);
            client.run.keep_size("m1", 100);
            batch.answer(batch_answer(&[(0, "403 Forbidden", body.clone())]));

            let err = client
                .fetch_bodies(&["m1"], &|_| None, |_| Ok(Vec::new()), |_, _, _| Ok(()))
                .unwrap_err();

            assert_eq!(Failure::of(&err).code, Some(code), "{body}: {err:#}");
        }
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
        let requests = batch.lines();
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
        let alone = api.lines();
        assert_eq!(alone.len(), 1, "{alone:?}");
        assert!(alone[0].contains("/messages/big"), "{alone:?}");
        assert!(alone[0].contains("format=RAW"), "{alone:?}");
        assert_eq!(batch.lines().len(), 1, "the small one by a batch");
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

    /// A push removing `\Flagged` from a message listed seen and flagged names
    /// only `STARRED` (pimdir SYNC §4).
    ///
    /// Another client marking it unread or important after the listing keeps it
    /// so.
    #[test]
    fn a_flag_push_names_only_the_labels_it_changes() {
        let (mut client, api, _) = scripted();
        api.answer(ok(serde_json::json!({ "id": "m1", "labelIds": ["INBOX"] })));

        let flagged = Flag::from_iana(IanaFlag::Flagged);
        client.update_flags(&["m1"], &[], &[flagged]).unwrap();

        let written = api.written(0);
        let body = written.rsplit("\r\n\r\n").next().unwrap_or_default();
        let modify: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(
            modify,
            serde_json::json!({ "addLabelIds": [], "removeLabelIds": ["STARRED"] })
        );
    }

    /// A write on a message gone already has nothing left to do.
    ///
    /// A batch modify answered 404 is done again one by one.
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
            .update_flags(&["m1"], &[Flag::from_iana(IanaFlag::Flagged)], &[])
            .unwrap();

        api.answer(gone());
        api.answer(ok(
            serde_json::json!({ "id": "m2", "labelIds": ["Label_1"] }),
        ));
        api.answer(gone());
        client.move_items("INBOX", "Work", &["m2", "m3"]).unwrap();
        let requests = api.lines();
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

    /// Gmail refuses `DRAFT` and `SENT` in any label change (`Invalid label`,
    /// 400, checked live on 2026-11-17).
    ///
    /// A delete from either trashes the message, naming `TRASH` alone; a move
    /// into or out of either but to the trash is refused before any request.
    #[test]
    fn drafts_and_sent_are_left_for_the_trash_alone() {
        let (mut client, api, _) = scripted();
        api.answer(ok(
            serde_json::json!({ "id": "m1", "labelIds": ["DRAFT", "TRASH"] }),
        ));

        client.delete_item("DRAFT", "m1").unwrap();

        let written = api.written(0);
        let body = written.rsplit("\r\n\r\n").next().unwrap_or_default();
        let modify: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(
            modify,
            serde_json::json!({ "addLabelIds": ["TRASH"], "removeLabelIds": [] })
        );

        for (from, to) in [("INBOX", "SENT"), ("SENT", "INBOX"), ("DRAFT", "INBOX")] {
            let err = client.move_items(from, to, &["m1"]).unwrap_err();
            let failure = crate::error::Failure::of(&err);
            assert_eq!(
                failure.code,
                Some(ErrorCode::Unsupported),
                "{from} to {to}: {err:#}"
            );
        }
        assert_eq!(api.lines().len(), 1, "{:?}", api.lines());
    }
}
