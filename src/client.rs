//! # Client seam
//!
//! The dispatching sync client and its per-side construction: [`Client`]
//! is a thin enum over the compiled-in backends, exposing the narrow
//! surface the sync engine needs and forwarding to each adapter.
//!
//! The seam is kind-neutral, speaking collections and items rather than
//! mailboxes and messages, which is what lets one DAV adapter serve
//! contacts and calendar alike; each adapter keeps its protocol vocabulary
//! behind it. [`Client::media_type`] reports which kind a backend syncs.
//!
//! The enumeration cursor is opaque here: each backend encodes its own
//! incremental-sync state into the checkpoint bytes the engine stores, and
//! member handles are strings (an IMAP UID in decimal, a Graph message id,
//! a DAV href). JMAP and Gmail configs parse but do not open yet.

use std::{
    collections::BTreeSet,
    io::{Read, Write},
};

use anyhow::{Result, bail};

#[cfg(feature = "dav")]
use crate::dav::client::DavClient;
#[cfg(feature = "imap")]
use crate::imap::client::ImapClient;
#[cfg(feature = "msgraph")]
use crate::msgraph::client::GraphClient;
use crate::{
    account::{SourceAccount, SourceAccountBackend},
    item::{collection::Collection, flag::Flag, flag::FlagOp, summary::ItemSummary},
    kind::LinkId,
};

/// A backend-neutral collection enumeration: the member+flag spine, plus
/// the opaque cursor a server-side incremental sync advances.
///
/// `complete` tells a full snapshot, where absence means removed, from a
/// delta, where `vanished` names the removals. Link ids resolve later at
/// the `Meta` tier, so an entry carries no summary.
pub struct Enumeration {
    /// The members the listing answered with, in the server's own order.
    pub items: Vec<EnumEntry>,
    /// The handles a delta reports as removed, empty on a full snapshot.
    pub vanished: Vec<String>,
    /// Whether the listing is the whole collection rather than a delta.
    pub complete: bool,
    /// The next sync's cursor, in the backend's own encoding.
    pub checkpoint: Vec<u8>,
}

/// One enumerated member: its handle and current flags.
///
/// A backend with no flag concept reports an empty set, which the engine
/// reads as known-empty and never as unknown.
pub struct EnumEntry {
    /// The member's handle on its own backend: an IMAP UID, a DAV href.
    pub id: String,
    /// The flags the backend currently reports on it.
    pub flags: BTreeSet<Flag>,
    /// The current content revision (a DAV ETag), on a mutable-content
    /// backend.
    ///
    /// `None` where content is immutable (IMAP, Graph), which io-replica's
    /// merge reads as unchanged, never as unknown.
    pub revision: Option<String>,
}

/// What a backend assigned to an item it just wrote.
pub struct WrittenItem {
    /// The server-assigned handle (an IMAP UID, a DAV href).
    pub id: String,
    /// The revision the remote now holds, `None` on immutable content.
    pub revision: Option<String>,
}

/// A live sync client: exactly one compiled-in backend per side.
pub enum Client {
    #[cfg(feature = "imap")]
    Imap(ImapClient),
    #[cfg(feature = "dav")]
    Dav(Box<DavClient>),
    #[cfg(feature = "msgraph")]
    Msgraph(Box<GraphClient>),
    /// Keeps the type inhabited when no backend is compiled in.
    ///
    /// Never constructed: [`open`] refuses every side first, so such a
    /// build fails when it opens a side, not when it builds.
    #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
    #[allow(dead_code)]
    Unavailable,
}

/// The error every method reports in a build with no backend at all.
#[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
const NO_BACKEND: &str =
    "No sync backend is compiled in (rebuild with the `imap`, `msgraph` or `dav` cargo feature)";

#[cfg_attr(
    not(all(feature = "imap", feature = "msgraph", feature = "dav")),
    allow(unused_variables)
)]
impl Client {
    /// Lists every selectable collection, counted when `with_counts`.
    pub fn list_collections(&mut self, with_counts: bool) -> Result<Vec<Collection>> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.list_mailboxes(with_counts),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) => c.list_mailboxes(with_counts),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.list_collections(with_counts),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Creates a collection. Pull-only on Graph (rejected).
    pub fn create_collection(&mut self, collection: &str) -> Result<()> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.create_mailbox(collection),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(_) => bail!("Graph mailboxes are pull-only (create not supported)"),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.create_collection(collection),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Deletes a collection. Pull-only on Graph (rejected).
    pub fn delete_collection(&mut self, collection: &str) -> Result<()> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.delete_mailbox(collection),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(_) => bail!("Graph mailboxes are pull-only (delete not supported)"),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.delete_collection(collection),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Enumerates a collection's member+flag spine, incrementally when the
    /// backend and `cursor` (the previous checkpoint) allow it.
    pub fn enumerate(&mut self, collection: &str, cursor: Option<&[u8]>) -> Result<Enumeration> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.enumerate(collection, cursor),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) => c.enumerate(collection, cursor),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.enumerate(collection, cursor),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Fetches the summaries of an id set, for `Meta`-tier link id and
    /// summary resolution, rather than listing the whole collection.
    pub fn fetch_summaries(&mut self, collection: &str, ids: &[&str]) -> Result<Vec<ItemSummary>> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.fetch_envelopes(collection, ids),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) => c.fetch_envelopes(collection, ids),
            #[cfg(feature = "dav")]
            Client::Dav(_) => bail!("DAV items have no summary tier (they resolve at Full)"),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Streams the bodies of `ids`, batched when the backend allows it,
    /// into a sink `open`ed as each item begins and `done` as it ends.
    ///
    /// No body lands in memory whole on the IMAP path. `done` also
    /// receives the revision the body corresponds to when the backend
    /// reports one (a DAV multiget), `None` on immutable content.
    pub fn fetch_bodies<S: Write>(
        &mut self,
        collection: &str,
        ids: &[&str],
        open: impl FnMut(&str) -> std::io::Result<S>,
        done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<()> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.fetch_bodies(collection, ids, open, done),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) => c.fetch_bodies(collection, ids, open, done),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.fetch_bodies(collection, ids, open, done),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Streams one item's raw body into `sink`, returning the revision it
    /// corresponds to when the backend reports one.
    ///
    /// The bytes are RFC 5322 for mail and a vCard or iCalendar object for
    /// the DAV kinds. On the IMAP path they never land in memory whole.
    pub fn get_item_stream(
        &mut self,
        collection: &str,
        id: &str,
        sink: impl Write,
    ) -> Result<Option<String>> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.get_message_stream(collection, id, sink).map(|()| None),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) => c.get_message_stream(collection, id, sink).map(|()| None),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.get_item_stream(collection, id, sink),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Adds an item streamed from `source` (`len` octets) with `flags`,
    /// returning the handle and revision the server assigned.
    ///
    /// `link`'s hint recovers the UID on IMAP servers lacking UIDPLUS and
    /// is the `UID` a DAV href is built from, while its mint keeps a second
    /// copy off the href its twin holds. Pull-only on Graph (rejected).
    pub fn add_item_stream(
        &mut self,
        collection: &str,
        flags: &[Flag],
        source: impl Read,
        len: usize,
        link: LinkId<'_>,
    ) -> Result<WrittenItem> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c
                .add_message_stream(collection, flags, source, len, link.hint)
                .map(|id| WrittenItem { id, revision: None }),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(_) => bail!("Graph messages are pull-only (append not supported)"),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.add_item_stream(collection, source, link),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Replaces an item's body in place, conditionally on `if_match` (the
    /// last-synced revision), returning the revision the remote now holds.
    ///
    /// Mutable-content backends only: a mail body is replaced by delete
    /// plus append and never edited, so both mail backends refuse this and
    /// io-replica never derives an `Update` for them.
    #[allow(unused_variables)]
    pub fn update_item_stream(
        &mut self,
        collection: &str,
        id: &str,
        source: impl Read,
        len: usize,
        if_match: Option<&str>,
    ) -> Result<Option<String>> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(_) => bail!("IMAP message bodies are immutable (in-place update)"),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(_) => bail!("Graph message bodies are immutable (in-place update)"),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.update_item_stream(collection, id, source, if_match),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Deletes one item, conditionally on `if_match` (the last-synced
    /// revision) where the backend supports it; IMAP and Graph ignore it.
    #[allow(unused_variables)]
    pub fn delete_item(
        &mut self,
        collection: &str,
        id: &str,
        if_match: Option<&str>,
    ) -> Result<()> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.delete_message(collection, id),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) => c.delete_message(id),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.delete_item(collection, id, if_match),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Moves an item set from `from` to `to`. Pull-only on Graph (rejected).
    pub fn move_items(&mut self, from: &str, to: &str, ids: &[&str]) -> Result<()> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.move_messages(from, to, ids),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(_) => bail!("Graph messages are pull-only (move not supported)"),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.move_items(from, to, ids),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Adds, sets or removes `flags` on an id set; Graph supports the
    /// full-set replace only.
    pub fn store_flags(
        &mut self,
        collection: &str,
        ids: &[&str],
        flags: &[Flag],
        op: FlagOp,
    ) -> Result<()> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.store_flags(collection, ids, flags, op),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) => c.store_flags(ids, flags, op),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.store_flags(ids, flags, op),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// The IANA media type of the items this backend syncs.
    ///
    /// Recorded as a collection's `kind` in the store, so the store is
    /// self-describing and may hold several kinds. The DAV adapter answers
    /// the flavour its session speaks, so one adapter describes two kinds.
    pub fn media_type(&self) -> &'static str {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(_) => "message/rfc822",
            #[cfg(feature = "msgraph")]
            Client::Msgraph(_) => "message/rfc822",
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.media_type(),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
            Client::Unavailable => "",
        }
    }

    /// The handle-space epoch a stored checkpoint carries: a counter that
    /// changes when the backend reassigns every handle (a UIDVALIDITY bump).
    ///
    /// A change means every cached handle is void, so the driver rebuilds
    /// the collection by link id; `None` is a backend that never rebuilds.
    /// It lives on the seam: only a backend reads its own checkpoint bytes.
    pub fn handle_space_epoch(&self, checkpoint: &[u8]) -> Option<u64> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(_) => {
                crate::imap::backend::checkpoint_uid_validity(checkpoint).map(u64::from)
            }
            #[cfg(feature = "msgraph")]
            Client::Msgraph(_) => None,
            #[cfg(feature = "dav")]
            Client::Dav(_) => None,
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
            Client::Unavailable => None,
        }
    }
}

/// Opens the protocol client `account` describes.
///
/// It spawns no process and reads no configuration, the credential being
/// already resolved for the run by [`crate::account`], so a second
/// connection to a side costs a handshake and nothing else.
#[cfg_attr(
    not(any(feature = "imap", feature = "msgraph", feature = "dav")),
    allow(unused_variables)
)]
pub fn open(account: &SourceAccount) -> Result<Client> {
    match &account.backend {
        #[cfg(feature = "imap")]
        SourceAccountBackend::Imap(imap) => {
            let client =
                ImapClient::connect(&imap.server, &imap.tls, imap.starttls, imap.sasl.clone())?;
            Ok(Client::Imap(client))
        }
        #[cfg(feature = "msgraph")]
        SourceAccountBackend::Msgraph(msgraph) => {
            let client =
                GraphClient::connect(&msgraph.token, &msgraph.user_id, msgraph.tls.clone())?;
            Ok(Client::Msgraph(Box::new(client)))
        }
        #[cfg(feature = "dav")]
        SourceAccountBackend::Dav(dav) => {
            let client = DavClient::connect(dav.kind, &dav.server, &dav.tls, dav.auth.clone())?;
            Ok(Client::Dav(Box::new(client)))
        }
        #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
        SourceAccountBackend::Unavailable => bail!(NO_BACKEND),
    }
}

/// Same as [`open`] plus any side-local bootstrap; none needs one today.
pub fn init(account: &SourceAccount) -> Result<Client> {
    open(account)
}

/// A side's persistent connection pool.
///
/// The primary is opened up front for the sequential operations; more are
/// opened lazily up to `max` for a concurrent `Full` fetch and kept for the
/// run. It holds a resolved [`SourceAccount`], so opening one spawns nothing.
pub struct Pool {
    account: SourceAccount,
    clients: Vec<Client>,
    max: usize,
}

impl Pool {
    /// Opens the pool with its primary connection, `max` clamped to one.
    pub fn open(account: SourceAccount, max: usize) -> Result<Self> {
        let primary = open(&account)?;
        Ok(Self {
            account,
            clients: vec![primary],
            max: max.max(1),
        })
    }

    /// The connection budget, the account's `connections` (default 4).
    pub fn max(&self) -> usize {
        self.max
    }

    /// The always-present primary connection, for sequential operations.
    pub fn primary(&mut self) -> &mut Client {
        &mut self.clients[0]
    }

    /// Up to `n` connections (capped at `max`) for a concurrent `Full`
    /// fetch, opening the missing ones and keeping them for the run.
    pub fn workers(&mut self, n: usize) -> Result<&mut [Client]> {
        let want = n.min(self.max);
        while self.clients.len() < want {
            self.clients.push(open(&self.account)?);
        }
        let take = want.min(self.clients.len());
        Ok(&mut self.clients[..take])
    }
}
