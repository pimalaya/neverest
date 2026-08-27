//! The dispatching sync client and its per-side construction.
//!
//! [`Client`] is a thin enum over the compiled-in backends (IMAP and
//! Microsoft Graph); it exposes the narrow method surface the sync
//! engine needs and forwards each call to the active backend's adapter
//! (the `crate::imap` and `crate::msgraph` submodules).
//!
//! **This seam is kind-neutral**: it speaks collections and items, never
//! mailboxes and messages, so a contacts or calendar backend implements
//! the same surface. Each adapter keeps its own protocol vocabulary
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

#[cfg(feature = "carddav")]
use crate::carddav::client::CarddavClient;
#[cfg(any(feature = "imap", feature = "msgraph", feature = "carddav"))]
use crate::config::SourceBackendConfig;
#[cfg(any(feature = "imap", feature = "carddav"))]
use crate::config::server_url;
#[cfg(feature = "imap")]
use crate::imap::client::ImapClient;
#[cfg(feature = "msgraph")]
use crate::msgraph::client::GraphClient;
use crate::{
    config::SourceConfig,
    item::{collection::Collection, flag::Flag, flag::FlagOp, summary::ItemSummary},
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
    #[cfg(feature = "carddav")]
    Carddav(Box<CarddavClient>),
    #[cfg(feature = "msgraph")]
    Msgraph(Box<GraphClient>),
    /// Keeps the type inhabited when no backend is compiled in. It is
    /// never constructed: [`open`] refuses every side first, so a build
    /// with no backend fails when it opens a side, not when it builds.
    #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
    #[allow(dead_code)]
    Unavailable,
}

/// The error every method reports in a build with no backend at all.
/// Unreachable in practice, [`open`] having refused the side already.
#[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
const NO_BACKEND: &str = "No sync backend is compiled in (rebuild with the `imap`, `msgraph` or `carddav` cargo feature)";

#[cfg_attr(
    not(all(feature = "imap", feature = "msgraph", feature = "carddav")),
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
            #[cfg(feature = "carddav")]
            Client::Carddav(c) => c.list_collections(with_counts),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
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
            #[cfg(feature = "carddav")]
            Client::Carddav(c) => c.create_collection(collection),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
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
            #[cfg(feature = "carddav")]
            Client::Carddav(c) => c.delete_collection(collection),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
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
            #[cfg(feature = "carddav")]
            Client::Carddav(c) => c.enumerate(collection, cursor),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
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
            #[cfg(feature = "carddav")]
            Client::Carddav(_) => {
                bail!("CardDAV cards have no summary tier (they resolve at Full)")
            }
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
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
            #[cfg(feature = "carddav")]
            Client::Carddav(c) => c.fetch_bodies(collection, ids, open, done),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
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
            #[cfg(feature = "carddav")]
            Client::Carddav(c) => c.get_item_stream(collection, id, sink),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Adds an item streamed from `source` (`len` octets) to `collection` with
    /// `flags`, returning the handle (and revision) the server assigned;
    /// `link_hint` recovers the UID on IMAP servers lacking UIDPLUS, and is the
    /// `UID` a DAV backend builds the new href from. Pull-only on Graph
    /// (rejected, so the engine records the push as rejected rather than
    /// mutating Graph).
    pub fn add_item_stream(
        &mut self,
        collection: &str,
        flags: &[Flag],
        source: impl Read,
        len: usize,
        link_hint: Option<&str>,
    ) -> Result<WrittenItem> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c
                .add_message_stream(collection, flags, source, len, link_hint)
                .map(|id| WrittenItem { id, revision: None }),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(_) => bail!("Graph messages are pull-only (append not supported)"),
            #[cfg(feature = "carddav")]
            Client::Carddav(c) => c.add_item_stream(collection, source, link_hint),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
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
            #[cfg(feature = "carddav")]
            Client::Carddav(c) => c.update_item_stream(collection, id, source, if_match),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
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
            #[cfg(feature = "carddav")]
            Client::Carddav(c) => c.delete_item(collection, id, if_match),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
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
            #[cfg(feature = "carddav")]
            Client::Carddav(c) => c.move_items(from, to, ids),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
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
            #[cfg(feature = "carddav")]
            Client::Carddav(c) => c.store_flags(ids, flags, op),
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// The IANA media type of the items this backend syncs — recorded as a
    /// collection's `kind` in the pimdir store so the store is self-describing
    /// and one store may hold several item kinds.
    ///
    /// Every backend compiled in today is mail (`message/rfc822`); a future
    /// contacts or calendar backend returns `text/vcard` / `text/calendar` from
    /// its own arm and the store records it with no further plumbing.
    pub fn media_type(&self) -> &'static str {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(_) => "message/rfc822",
            #[cfg(feature = "msgraph")]
            Client::Msgraph(_) => "message/rfc822",
            #[cfg(feature = "carddav")]
            Client::Carddav(_) => "text/vcard",
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
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
            #[cfg(feature = "carddav")]
            Client::Carddav(_) => None,
            #[cfg(not(any(feature = "imap", feature = "msgraph", feature = "carddav")))]
            Client::Unavailable => None,
        }
    }
}

/// Opens the protocol client for `config`, resolving its configured
/// secrets (passwords, bearer token commands) once per opened client.
pub fn open(config: SourceConfig) -> Result<Client> {
    match config.backend {
        #[cfg(feature = "imap")]
        SourceBackendConfig::Imap(config) => {
            let alpn = config.alpn.unwrap_or_else(io_imap::client::default_alpn);
            let tls = config.tls.into_tls(alpn);

            let server = server_url(&config.server, "imaps")?;

            let sasl = config
                .sasl
                .map(|cfg| {
                    let host = server.host_str().unwrap_or_default();
                    let port = server
                        .port()
                        .unwrap_or_else(|| io_imap::client::default_port(server.scheme()));
                    cfg.try_into_sasl(host, port)
                })
                .transpose()?;

            let client = ImapClient::connect(&server, &tls, config.starttls, sasl)?;
            Ok(Client::Imap(client))
        }
        #[cfg(feature = "msgraph")]
        SourceBackendConfig::Msgraph(config) => {
            let token = config.auth.token.get()?;
            let tls = config.tls.into_tls(config.alpn);
            let client = GraphClient::connect(&token, &config.user_id, tls)?;
            Ok(Client::Msgraph(Box::new(client)))
        }
        #[cfg(feature = "carddav")]
        SourceBackendConfig::Carddav(config) => {
            let tls = config.tls.into_tls(config.alpn);
            let server = server_url(&config.server, "https")?;
            let auth = config.auth.try_into_dav_auth()?;
            let client = CarddavClient::connect(&server, &tls, auth)?;
            Ok(Client::Carddav(Box::new(client)))
        }
        #[allow(unreachable_patterns)]
        _ => bail!(
            "This side's backend is not available in this build (rebuild with the matching cargo feature; only imap, msgraph and carddav have a backend for now)"
        ),
    }
}

/// Same as [`open`] plus any side-local bootstrap. No compiled-in
/// backend needs one.
pub fn init(config: SourceConfig) -> Result<Client> {
    open(config)
}

/// A side's persistent connection pool.
///
/// One connection (the primary) is opened up front for the sequential
/// operations (list, enumerate, push, meta); more are opened lazily, up to
/// `max`, when a `Full` fetch streams several bodies at once, and then kept for
/// the rest of the run so their auth is paid once, not per batch. `max` is the
/// account's connection budget (default 4), kept under the server's per-account
/// cap.
pub struct Pool {
    config: SourceConfig,
    clients: Vec<Client>,
    max: usize,
}

impl Pool {
    /// Opens the pool with its primary connection; `max` is clamped to at least
    /// one.
    pub fn open(config: SourceConfig, max: usize) -> Result<Self> {
        let primary = open(config.clone())?;
        Ok(Self {
            config,
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
            self.clients.push(open(self.config.clone())?);
        }
        let take = want.min(self.clients.len());
        Ok(&mut self.clients[..take])
    }
}
