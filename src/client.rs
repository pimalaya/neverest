//! The dispatching sync client and its per-side construction.
//!
//! [`Client`] is a thin enum over the compiled-in backends (IMAP,
//! Microsoft Graph and DAV); it exposes the narrow method surface the
//! sync engine needs and forwards each call to the active backend's
//! adapter (the `crate::imap`, `crate::msgraph` and `crate::dav`
//! submodules).
//!
//! **This seam is kind-neutral**: it speaks collections and items, never
//! mailboxes and messages, which is what lets one DAV adapter serve
//! contacts and calendar alike. Each adapter keeps its own protocol vocabulary
//! behind it (an IMAP mailbox stays a mailbox inside `crate::imap`) and
//! converts at the edge. Which kind a backend syncs is reported by
//! [`Client::media_type`].
//!
//! The JMAP and Gmail side configs still parse, but opening them is not
//! yet supported in this build — they will return on their own lean
//! backends over time.
//!
//! The enumeration cursor is opaque to this seam: each backend encodes
//! its own incremental-sync state into the checkpoint bytes the engine
//! stores (IMAP its `(UIDVALIDITY, HIGHESTMODSEQ)` pair, Graph its
//! `@odata.deltaLink`, a DAV backend its sync token), and member handles
//! are strings (an IMAP UID rendered in decimal, a Graph message id
//! verbatim, a DAV href).

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

/// A backend-neutral collection enumeration: the member+flag spine plus the
/// opaque cursor a server-side incremental sync (IMAP QRESYNC/CONDSTORE,
/// the Graph messages delta, a DAV `sync-collection` token) advances.
///
/// `complete` distinguishes a full snapshot (the whole member set; absence means
/// removed) from a delta (only changed members, with `vanished` naming removals).
/// The link id is resolved later at the `Meta` tier, so an entry carries no
/// summary.
pub struct Enumeration {
    pub items: Vec<EnumEntry>,
    pub vanished: Vec<String>,
    pub complete: bool,
    /// The next sync's cursor, in the backend's own encoding; stored
    /// verbatim as the engine checkpoint.
    pub checkpoint: Vec<u8>,
}

/// One enumerated member: its handle (an IMAP UID in decimal, a Graph
/// message id, a DAV href) and current flags. A backend with no flag
/// concept reports an empty set — *known-empty*, never unknown.
pub struct EnumEntry {
    pub id: String,
    pub flags: BTreeSet<Flag>,
    /// The current content revision (a DAV ETag), for a backend whose item
    /// bodies change in place. `None` where content is immutable (IMAP,
    /// Graph), which io-replica's merge reads as *unchanged*, never as
    /// unknown.
    pub revision: Option<String>,
}

/// What a backend assigned to an item it just wrote.
pub struct WrittenItem {
    /// The server-assigned handle (an IMAP UID, a DAV href).
    pub id: String,
    /// The content revision the remote now holds, when it reports one;
    /// `None` on an immutable-content backend.
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
    /// Keeps the type inhabited when no backend is compiled in. It is
    /// never constructed: [`open`] refuses every side first, so a build
    /// with no backend fails when it opens a side, not when it builds.
    #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
    #[allow(dead_code)]
    Unavailable,
}

/// The error every method reports in a build with no backend at all.
/// Unreachable in practice, [`open`] having refused the side already.
#[cfg(not(any(feature = "imap", feature = "msgraph", feature = "dav")))]
const NO_BACKEND: &str =
    "No sync backend is compiled in (rebuild with the `imap`, `msgraph` or `dav` cargo feature)";

#[cfg_attr(
    not(all(feature = "imap", feature = "msgraph", feature = "dav")),
    allow(unused_variables)
)]
impl Client {
    /// Lists every selectable collection (with totals/unread when
    /// `with_counts`).
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

    /// Enumerates a collection's member+flag spine, incrementally when the backend
    /// and `cursor` (the previous run's opaque checkpoint bytes) allow it.
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

    /// Fetches item summaries for a specific id set (targeted, for `Meta`-tier link
    /// id / summary resolution) rather than listing the whole collection.
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

    /// Streams the bodies of `ids`, batched when the backend allows it (one
    /// IMAP `UID FETCH … (UID BODY.PEEK[])` for the whole set, one DAV multiget;
    /// one raw MIME get per message on Graph), routing each to a sink `open`ed
    /// when its item begins and `done` when it ends; no body lands in memory
    /// whole on the IMAP path.
    ///
    /// `done` receives the content revision the body corresponds to, when the
    /// backend reports one alongside it (a DAV multiget returns the ETag with
    /// each object); `None` on an immutable-content backend.
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

    /// Streams one item's raw body bytes into `sink` (RFC 5322 for mail, a
    /// vCard or iCalendar object for the DAV kinds), returning the content
    /// revision that body corresponds to when the backend reports one; on the
    /// IMAP path the body never lands in memory whole.
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

    /// Adds an item streamed from `source` (`len` octets) to `collection` with
    /// `flags`, returning the handle (and revision) the server assigned.
    ///
    /// `link` is the item's key as
    /// [`Kind::split_link_id`](crate::kind::Kind::split_link_id) reads it: its
    /// hint recovers the UID on IMAP servers lacking UIDPLUS, and is the `UID`
    /// a DAV backend builds the new href from, while its mint is what keeps
    /// the href of a second copy off the href its twin already holds.
    /// Pull-only on Graph (rejected, so the engine records the push as rejected
    /// rather than mutating Graph).
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

    /// Replaces an item's body in place with `source` (`len` octets),
    /// conditionally on `if_match` (the last-synced revision), returning the
    /// revision the remote now holds.
    ///
    /// **Mutable-content backends only.** Mail bodies are immutable — a message
    /// is replaced by delete + append, never edited — so both mail backends
    /// refuse this, and io-replica never derives an `Update` for them.
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

    /// Deletes one item from `collection`, conditionally on `if_match` (the
    /// last-synced revision) where the backend supports it. IMAP and Graph have
    /// no such precondition and ignore it.
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

    /// Adds, sets or removes `flags` on an id set in `collection`. Graph supports
    /// the full-set replace only (`isRead` / follow-up flagStatus).
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

    /// The IANA media type of the items this backend syncs — recorded as a
    /// collection's `kind` in the pimdir store so the store is self-describing
    /// and one store may hold several item kinds.
    ///
    /// The mail backends answer it outright; the DAV one answers the flavour
    /// its session speaks, `text/vcard` for CardDAV and `text/calendar` for
    /// CalDAV, so one adapter still describes two kinds.
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

    /// The **handle-space epoch** a stored checkpoint carries, if this backend
    /// has one: an opaque counter that changes when the backend discards and
    /// reassigns every handle in a collection (an IMAP UIDVALIDITY bump).
    ///
    /// The driver compares it before and after a pull; a change means every
    /// cached handle is void, so the collection is rebuilt by link id and its
    /// pimdir generation bumped. `None` means the backend has no such notion
    /// and never rebuilds — Graph message ids survive a delta reset, and a DAV
    /// href is stable for the life of the resource.
    ///
    /// The checkpoint bytes are the backend's own encoding, so only the
    /// backend can read them; that is why this lives on the seam rather than
    /// the driver decoding an IMAP cursor it should know nothing about.
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
/// It spawns no process and reads no configuration: the credential is
/// already in hand, resolved once for the run by [`crate::account`], so
/// opening a second connection to a side costs a handshake and nothing
/// else.
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

/// Same as [`open`] plus any side-local bootstrap. No compiled-in
/// backend needs one.
pub fn init(account: &SourceAccount) -> Result<Client> {
    open(account)
}

/// A side's persistent connection pool.
///
/// One connection (the primary) is opened up front for the sequential
/// operations (list, enumerate, push, meta); more are opened lazily, up to
/// `max`, when a `Full` fetch streams several bodies at once, and then kept for
/// the rest of the run so their auth is paid once, not per batch. `max` is the
/// account's connection budget (default 4), kept under the server's per-account
/// cap.
///
/// The pool holds the resolved [`SourceAccount`] rather than the
/// configuration it came from, which is what makes a lazily-opened
/// connection free of everything but its handshake: there is no command
/// left to spawn.
pub struct Pool {
    account: SourceAccount,
    clients: Vec<Client>,
    max: usize,
}

impl Pool {
    /// Opens the pool with its primary connection; `max` is clamped to at least
    /// one.
    pub fn open(account: SourceAccount, max: usize) -> Result<Self> {
        let primary = open(&account)?;
        Ok(Self {
            account,
            clients: vec![primary],
            max: max.max(1),
        })
    }

    /// The connection budget (its `max`).
    pub fn max(&self) -> usize {
        self.max
    }

    /// The always-present primary connection, for sequential operations.
    pub fn primary(&mut self) -> &mut Client {
        &mut self.clients[0]
    }

    /// Up to `n` connections (capped at `max`) for a concurrent `Full` fetch,
    /// opening any missing ones lazily and keeping them for the rest of the run.
    pub fn workers(&mut self, n: usize) -> Result<&mut [Client]> {
        let want = n.min(self.max);
        while self.clients.len() < want {
            self.clients.push(open(&self.account)?);
        }
        let take = want.min(self.clients.len());
        Ok(&mut self.clients[..take])
    }
}
