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
//! a DAV href, a Gmail message id). JMAP configs parse but do not open yet.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    io::{Read, Write},
};

use anyhow::{Result, bail};
use io_pimdir::{collection::PimdirScope, remote::PimdirEnumerate, summary::PimdirDerivation};

#[cfg(feature = "dav")]
use crate::dav::client::DavClient;
#[cfg(feature = "gcal")]
use crate::gcal::client::GcalClient;
#[cfg(feature = "gmail")]
use crate::gmail::client::GmailClient;
#[cfg(feature = "gpeople")]
use crate::gpeople::client::GpeopleClient;
#[cfg(feature = "imap")]
use crate::imap::client::ImapClient;
#[cfg(feature = "msgraph")]
use crate::msgraph::client::{GraphClient, GraphKind};
use crate::{
    account::{SourceAccount, SourceAccountBackend},
    item::{collection::Collection, flag::Flag, flag::FlagOp, summary::ItemSummary},
    kind::LinkId,
    offline::invitation::Refusal,
};

/// One page of a backend-neutral enumeration (pimdir SYNC §4).
///
/// `complete` tells a page of a round, where absence across the round's
/// pages means removed in scope, from a delta, where `vanished` names the
/// removals. A round of one page (no `cursor`) is the whole listing of a
/// backend that does not page. Every entry carries what names it, read in
/// the listing itself, unless the store already holds it or its kind is
/// named by its body, which [`crate::offline::remote::PimRemote`] fetches.
#[derive(Debug, Default)]
pub struct Enumeration {
    /// The members the page answered with, in the server's own order.
    pub items: Vec<EnumEntry>,
    /// The handles the backend states removed, applied whatever the scope.
    pub vanished: Vec<String>,
    /// Whether the page belongs to a round rather than a delta.
    pub complete: bool,
    /// The resume cursor, on every page of a round but the last.
    pub cursor: Option<Vec<u8>>,
    /// The next sync's checkpoint, in the backend's own encoding, on the
    /// page the backend gives it: a delta's, a round's first (IMAP
    /// `HIGHESTMODSEQ`, Gmail's `historyId`) or its last (a Graph delta
    /// link).
    pub checkpoint: Option<Vec<u8>>,
    /// The octets of meta the page carried (headers, summaries), for the
    /// source's download count.
    pub bytes: u64,
}

#[cfg_attr(
    not(all(
        feature = "imap",
        feature = "msgraph",
        feature = "dav",
        feature = "gpeople",
        feature = "gcal",
        feature = "gmail"
    )),
    allow(dead_code)
)]
impl Enumeration {
    /// A round of one page: every member, and the checkpoint.
    pub fn round(items: Vec<EnumEntry>, checkpoint: Vec<u8>) -> Self {
        Self {
            items,
            complete: true,
            checkpoint: Some(checkpoint),
            ..Default::default()
        }
    }

    /// A delta: what changed, what was removed, and the next checkpoint.
    pub fn delta(items: Vec<EnumEntry>, vanished: Vec<String>, checkpoint: Vec<u8>) -> Self {
        Self {
            items,
            vanished,
            checkpoint: Some(checkpoint),
            ..Default::default()
        }
    }
}

/// What a backend answered an enumeration with.
#[derive(Debug)]
#[cfg_attr(
    not(all(
        feature = "imap",
        feature = "msgraph",
        feature = "dav",
        feature = "gpeople",
        feature = "gcal",
        feature = "gmail"
    )),
    allow(dead_code)
)]
pub enum Listed {
    /// One page of the listing asked for.
    Page(Enumeration),
    /// The backend refused the resume cursor or the checkpoint (an expired
    /// Graph link, a Gmail page token): the engine restarts the round, or
    /// opens one where a delta's checkpoint was refused.
    CursorRejected,
}

/// What the store already binds of a collection, which a mail listing reads
/// no meta for: a message, immutable, bound once is never named again.
#[derive(Clone, Copy)]
#[cfg_attr(
    not(all(
        feature = "imap",
        feature = "msgraph",
        feature = "dav",
        feature = "gpeople",
        feature = "gcal",
        feature = "gmail"
    )),
    allow(dead_code)
)]
pub struct Held<'a> {
    /// The handles this source binds in the collection.
    pub handles: &'a HashSet<String>,
    /// The checkpoint the store holds, which tells a backend whose handles
    /// can be reassigned (an IMAP `UIDVALIDITY`) whether they still name
    /// what they named.
    pub checkpoint: Option<&'a [u8]>,
    /// The scope of the source's last closed round, which tells a Graph
    /// mail source whether a delta link stored before it tagged its links
    /// was made under a `$filter`.
    pub coverage: Option<&'a PimdirScope>,
    /// The bound messages with no date, by their last-synced flags: in
    /// every scope, a band round included, which a Graph band listing on
    /// `sentDateTime` never lists.
    pub undated: &'a HashMap<String, BTreeSet<Flag>>,
}

#[cfg_attr(
    not(all(
        feature = "imap",
        feature = "msgraph",
        feature = "dav",
        feature = "gpeople",
        feature = "gcal",
        feature = "gmail"
    )),
    allow(dead_code)
)]
impl Held<'_> {
    /// Whether the store binds `handle`.
    pub fn holds(&self, handle: &str) -> bool {
        self.handles.contains(handle)
    }
}

/// One enumerated member: its handle, current flags and what names it.
///
/// A backend with no flag concept reports an empty set, which the engine
/// reads as known-empty and never as unknown.
#[derive(Debug)]
pub struct EnumEntry {
    /// The member's handle on its own backend: an IMAP UID, a DAV href.
    pub id: String,
    /// The flags the backend currently reports on it.
    pub flags: BTreeSet<Flag>,
    /// The current content revision (a DAV ETag), on a mutable-content
    /// backend.
    ///
    /// `None` where content is immutable (IMAP, Graph), which io-pimdir's
    /// merge reads as unchanged, never as unknown.
    pub revision: Option<String>,
    /// The identity hint, summary and sort key the listing read (pimdir
    /// STORAGE Annex A): `None` for a member the store already binds, or
    /// one of a kind whose body is what names it.
    pub meta: Option<PimdirDerivation>,
}

#[cfg_attr(
    not(all(
        feature = "imap",
        feature = "msgraph",
        feature = "dav",
        feature = "gpeople",
        feature = "gcal",
        feature = "gmail"
    )),
    allow(dead_code)
)]
impl EnumEntry {
    /// A member named by its body, or one the store already binds.
    pub fn bare(id: String, flags: BTreeSet<Flag>, revision: Option<String>) -> Self {
        Self {
            id,
            flags,
            revision,
            meta: None,
        }
    }
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
    Imap(Box<ImapClient>),
    #[cfg(feature = "dav")]
    Dav(Box<DavClient>),
    #[cfg(feature = "msgraph")]
    Msgraph(Box<GraphClient>),
    #[cfg(feature = "gpeople")]
    Gpeople(Box<GpeopleClient>),
    #[cfg(feature = "gcal")]
    Gcal(Box<GcalClient>),
    #[cfg(feature = "gmail")]
    Gmail(Box<GmailClient>),
    /// Keeps the type inhabited when no backend is compiled in.
    ///
    /// Never constructed: [`open`] refuses every side first, so such a
    /// build fails when it opens a side, not when it builds.
    #[cfg(not(any(
        feature = "imap",
        feature = "msgraph",
        feature = "dav",
        feature = "gpeople",
        feature = "gcal",
        feature = "gmail"
    )))]
    #[allow(dead_code)]
    Unavailable,
}

/// The error every method reports in a build with no backend at all.
#[cfg(not(any(
    feature = "imap",
    feature = "msgraph",
    feature = "dav",
    feature = "gpeople",
    feature = "gcal",
    feature = "gmail"
)))]
const NO_BACKEND: &str =
    "No sync backend is compiled in (rebuild with a backend cargo feature such as `imap`)";

#[cfg_attr(
    not(all(
        feature = "imap",
        feature = "msgraph",
        feature = "dav",
        feature = "gpeople",
        feature = "gcal",
        feature = "gmail"
    )),
    allow(unused_variables)
)]
impl Client {
    /// Lists every selectable collection, counted when `with_counts`.
    pub fn list_collections(&mut self, with_counts: bool) -> Result<Vec<Collection>> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.list_mailboxes(with_counts),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) => c.list_collections(with_counts),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.list_collections(with_counts),
            #[cfg(feature = "gpeople")]
            Client::Gpeople(c) => c.list_collections(),
            #[cfg(feature = "gcal")]
            Client::Gcal(c) => c.list_collections(),
            #[cfg(feature = "gmail")]
            Client::Gmail(c) => c.list_collections(with_counts),
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// The role each collection states on the server (`inbox`, `sent`,
    /// `drafts`, `trash`, `junk`, `all`, `archive`), by collection id.
    ///
    /// What `check` prints; a sync reads [`Collection::role`] and
    /// [`Client::lookup_roles`] instead. Backends without mail roles (DAV,
    /// Google Calendar and People) answer none.
    pub fn collection_roles(&mut self) -> Result<BTreeMap<String, String>> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.mailbox_roles(),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) => c.mailbox_roles(),
            #[cfg(feature = "gmail")]
            Client::Gmail(c) => c.mailbox_roles(),
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
            Client::Unavailable => bail!(NO_BACKEND),
            #[allow(unreachable_patterns)]
            _ => Ok(BTreeMap::new()),
        }
    }

    /// The collection a new item goes to when none is named, by collection
    /// id: Google's primary calendar, Graph's default calendar and default
    /// Contacts folder, the one People address book, the CalDAV calendar
    /// the scheduling inbox names (RFC 6638), when the server gives it.
    ///
    /// A check-only probe, never stored. Mail (whose folders have roles)
    /// and CardDAV answer none.
    pub fn default_collection(&mut self) -> Result<Option<String>> {
        match self {
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) => c.default_collection(),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.default_collection(),
            #[cfg(feature = "gpeople")]
            Client::Gpeople(c) => Ok(c.default_collection()),
            #[cfg(feature = "gcal")]
            Client::Gcal(c) => c.default_collection(),
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
            Client::Unavailable => bail!(NO_BACKEND),
            #[allow(unreachable_patterns)]
            _ => Ok(None),
        }
    }

    /// The roles a listing does not carry, which cost requests of their
    /// own, by collection id: Graph's well-known mail folders (one lookup
    /// each) and the CalDAV default calendar (two `PROPFIND`s). `None` for
    /// a backend whose listing states every role it has ([`Collection::role`]).
    ///
    /// A sync asks only when the source's collection set moved since it
    /// last asked, and keeps the answer in the store's sidecar.
    pub fn lookup_roles(&mut self) -> Result<Option<BTreeMap<String, String>>> {
        match self {
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) if c.kind() == GraphKind::Mail => c.mailbox_roles().map(Some),
            #[cfg(feature = "dav")]
            Client::Dav(c) => Ok(Some(
                c.default_collection()?
                    .map(|id| (id, String::from("default")))
                    .into_iter()
                    .collect(),
            )),
            #[allow(unreachable_patterns)]
            _ => Ok(None),
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
            #[cfg(feature = "gpeople")]
            Client::Gpeople(_) => {
                bail!("Google address books and calendars are pull-only (create not supported)")
            }
            #[cfg(feature = "gcal")]
            Client::Gcal(_) => {
                bail!("Google address books and calendars are pull-only (create not supported)")
            }
            #[cfg(feature = "gmail")]
            Client::Gmail(c) => c.create_collection(collection),
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Creates the collection `name` under `parent`, the top level when
    /// `None`, and answers the id it is listed under; one the server
    /// already lists there is success (pimdir Annex B.2
    /// `collection-create`).
    ///
    /// IMAP joins the two with the parent's hierarchy delimiter, Gmail
    /// nests a label by its name (`Parent/name`), and DAV, whose
    /// collections do not nest, keys a new one by a path segment made from
    /// `name` and displays it as `name`. Graph and Google Calendar and
    /// People create none. A request no run can perform fails with a
    /// [`Refusal`].
    pub fn create_named_collection(&mut self, parent: Option<&str>, name: &str) -> Result<String> {
        let refused = |why: String| Err(anyhow::Error::new(Refusal(why)));

        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => {
                let id = match parent {
                    Some(parent) => c.child_mailbox(parent, name)?,
                    None => name.to_owned(),
                };
                let listed = c.list_mailboxes(false)?;
                if !listed.iter().any(|collection| collection.id == id) {
                    c.create_mailbox(&id)?;
                }
                Ok(id)
            }
            #[cfg(feature = "gmail")]
            Client::Gmail(c) => c.create_child_label(parent, name),
            #[cfg(feature = "dav")]
            Client::Dav(c) => {
                if let Some(parent) = parent {
                    return refused(format!(
                        "DAV collections do not nest, {name} cannot go under {parent}"
                    ));
                }
                let listed = c.list_collections(false)?;
                if let Some(found) = listed
                    .iter()
                    .find(|collection| collection.name == name || collection.id == name)
                {
                    return Ok(found.id.clone());
                }
                let id = free_segment(name, &listed);
                c.create_collection_named(&id, name)?;
                Ok(id)
            }
            #[allow(unreachable_patterns)]
            _ => refused(format!(
                "This source creates no collection, so {name} cannot be created"
            )),
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
            #[cfg(feature = "gpeople")]
            Client::Gpeople(_) => {
                bail!("Google address books and calendars are pull-only (delete not supported)")
            }
            #[cfg(feature = "gcal")]
            Client::Gcal(_) => {
                bail!("Google address books and calendars are pull-only (delete not supported)")
            }
            #[cfg(feature = "gmail")]
            Client::Gmail(c) => c.delete_collection(collection),
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Lists one page of a collection (pimdir SYNC §4): of a delta from the
    /// checkpoint `request` holds, or of a round over its scope, from its
    /// start or its cursor. `held` is what the store binds already, which a
    /// mail listing names from the store rather than reading its meta again.
    ///
    /// A backend that does not page answers a round in one page, and one
    /// whose kind is named by its body (DAV, Google Calendar and People,
    /// Graph contacts and events) answers bare entries.
    pub fn enumerate(
        &mut self,
        collection: &str,
        request: &PimdirEnumerate,
        held: Held<'_>,
    ) -> Result<Listed> {
        let checkpoint = request
            .checkpoint()
            .map(|checkpoint| checkpoint.0.as_slice());

        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.enumerate(collection, request, held),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) => c.enumerate(collection, request, held),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.enumerate(collection, checkpoint).map(Listed::Page),
            #[cfg(feature = "gpeople")]
            Client::Gpeople(c) => c.enumerate(collection, checkpoint).map(Listed::Page),
            #[cfg(feature = "gcal")]
            Client::Gcal(c) => c.enumerate(collection, checkpoint).map(Listed::Page),
            #[cfg(feature = "gmail")]
            Client::Gmail(c) => c.enumerate(collection, request, held),
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Whether this backend's checkpoint is bound to the scope it was made
    /// under (pimdir SYNC §4): IMAP's modseq, Gmail's history and Graph
    /// mail's delta link, made over the whole folder and checked locally,
    /// are not, and widen a scope by listing only the band their coverage
    /// lacks. The kinds that take no scope keep the default.
    pub fn scope_bound(&self) -> bool {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(_) => false,
            #[cfg(feature = "gmail")]
            Client::Gmail(_) => false,
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) => c.kind() != GraphKind::Mail,
            #[allow(unreachable_patterns)]
            _ => true,
        }
    }

    /// Fetches the summaries of an id set, for `Meta`-tier link id and
    /// summary resolution, rather than listing the whole collection.
    pub fn fetch_summaries(&mut self, collection: &str, ids: &[&str]) -> Result<Vec<ItemSummary>> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.fetch_envelopes(collection, ids),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) => c.fetch_summaries(collection, ids),
            #[cfg(feature = "dav")]
            Client::Dav(_) => bail!("DAV items have no summary tier (they resolve at Full)"),
            #[cfg(feature = "gpeople")]
            Client::Gpeople(_) => {
                bail!("Google contacts have no summary tier (they resolve at Full)")
            }
            #[cfg(feature = "gcal")]
            Client::Gcal(_) => bail!("Google events have no summary tier (they resolve at Full)"),
            #[cfg(feature = "gmail")]
            Client::Gmail(c) => c.fetch_summaries(ids),
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Streams the bodies of `ids` into a sink `open`ed and `done` per item.
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
            #[cfg(feature = "gpeople")]
            Client::Gpeople(c) => c.fetch_bodies(collection, ids, open, done),
            #[cfg(feature = "gcal")]
            Client::Gcal(c) => c.fetch_bodies(collection, ids, open, done),
            #[cfg(feature = "gmail")]
            Client::Gmail(c) => c.fetch_bodies(ids, open, done),
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
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
            Client::Msgraph(c) => c.get_item_stream(collection, id, sink),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.get_item_stream(collection, id, sink),
            #[cfg(feature = "gpeople")]
            Client::Gpeople(c) => c.get_item_stream(collection, id, sink),
            #[cfg(feature = "gcal")]
            Client::Gcal(c) => c.get_item_stream(collection, id, sink),
            #[cfg(feature = "gmail")]
            Client::Gmail(c) => c.get_item_stream(id, sink),
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Adds an item from `source` (`len` octets), returning what was assigned.
    ///
    /// `link`'s hint recovers the UID on IMAP servers lacking UIDPLUS and
    /// is the `UID` a DAV href is built from, while its mint keeps a second
    /// copy off the href its twin holds. Graph assigns its own ids, and takes
    /// a message into Drafts only, creating every MIME message as a draft.
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
            Client::Msgraph(c) => c.add_item_stream(collection, flags, source),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.add_item_stream(collection, source, link),
            #[cfg(feature = "gpeople")]
            Client::Gpeople(c) => c.add_item_stream(collection, source),
            #[cfg(feature = "gcal")]
            Client::Gcal(c) => c.add_item_stream(collection, source),
            #[cfg(feature = "gmail")]
            Client::Gmail(c) => c.add_item_stream(collection, flags, source, link.hint),
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Replaces a body in place on `if_match`, returning the new revision.
    ///
    /// Mutable-content backends only: a mail body is replaced by delete
    /// plus append and never edited, so both mail backends refuse this and
    /// io-pimdir never derives an `Update` for them.
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
            Client::Msgraph(c) => c.update_item_stream(id, source, if_match),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.update_item_stream(collection, id, source, if_match),
            #[cfg(feature = "gpeople")]
            Client::Gpeople(c) => c.update_item_stream(id, source, if_match),
            #[cfg(feature = "gcal")]
            Client::Gcal(c) => c.update_item_stream(collection, id, source, if_match),
            #[cfg(feature = "gmail")]
            Client::Gmail(_) => bail!("Gmail message bodies are immutable (in-place update)"),
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
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
            Client::Msgraph(c) => c.delete_item(id, if_match),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.delete_item(collection, id, if_match),
            #[cfg(feature = "gpeople")]
            Client::Gpeople(c) => c.delete_item(id, if_match),
            #[cfg(feature = "gcal")]
            Client::Gcal(c) => c.delete_item(collection, id, if_match),
            #[cfg(feature = "gmail")]
            Client::Gmail(c) => c.delete_item(collection, id),
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
            Client::Unavailable => bail!(NO_BACKEND),
        }
    }

    /// Copies one item from `from` to `to` server side, returning the copy,
    /// or `None` for a backend that uploads the body instead.
    ///
    /// Graph mail copies: an upload would land a draft, and Drafts only.
    #[allow(unused_variables)]
    pub fn copy_item(&mut self, from: &str, to: &str, id: &str) -> Result<Option<WrittenItem>> {
        match self {
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) if c.kind() == GraphKind::Mail => c.copy_message(to, id).map(Some),
            _ => Ok(None),
        }
    }

    /// Moves an item set from `from` to `to`.
    pub fn move_items(&mut self, from: &str, to: &str, ids: &[&str]) -> Result<()> {
        match self {
            #[cfg(feature = "imap")]
            Client::Imap(c) => c.move_messages(from, to, ids),
            #[cfg(feature = "msgraph")]
            Client::Msgraph(c) => c.move_messages(to, ids),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.move_items(from, to, ids),
            #[cfg(feature = "gpeople")]
            Client::Gpeople(_) => bail!("Google contacts cannot move (move not supported)"),
            #[cfg(feature = "gcal")]
            Client::Gcal(_) => {
                bail!("Google events cannot move between calendars here (move not supported)")
            }
            #[cfg(feature = "gmail")]
            Client::Gmail(c) => c.move_items(from, to, ids),
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
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
            #[cfg(feature = "gpeople")]
            Client::Gpeople(c) => c.store_flags(ids, flags, op),
            #[cfg(feature = "gcal")]
            Client::Gcal(c) => c.store_flags(ids, flags, op),
            #[cfg(feature = "gmail")]
            Client::Gmail(c) => c.store_flags(ids, flags, op),
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
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
            Client::Msgraph(c) => c.kind().media_type(),
            #[cfg(feature = "dav")]
            Client::Dav(c) => c.media_type(),
            #[cfg(feature = "gpeople")]
            Client::Gpeople(_) => "text/vcard",
            #[cfg(feature = "gcal")]
            Client::Gcal(_) => "text/calendar",
            #[cfg(feature = "gmail")]
            Client::Gmail(_) => "message/rfc822",
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
            Client::Unavailable => "",
        }
    }

    /// The epoch a checkpoint carries, changing when handles are reassigned.
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
            #[cfg(feature = "gpeople")]
            Client::Gpeople(_) => None,
            #[cfg(feature = "gcal")]
            Client::Gcal(_) => None,
            #[cfg(feature = "gmail")]
            Client::Gmail(_) => None,
            #[cfg(not(any(
                feature = "imap",
                feature = "msgraph",
                feature = "dav",
                feature = "gpeople",
                feature = "gcal",
                feature = "gmail"
            )))]
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
    not(any(
        feature = "imap",
        feature = "msgraph",
        feature = "dav",
        feature = "gpeople",
        feature = "gcal",
        feature = "gmail"
    )),
    allow(unused_variables)
)]
pub fn open(account: &SourceAccount) -> Result<Client> {
    match &account.backend {
        #[cfg(feature = "imap")]
        SourceAccountBackend::Imap(imap) => {
            let client =
                ImapClient::connect(&imap.server, &imap.tls, imap.starttls, imap.sasl.clone())?;
            Ok(Client::Imap(Box::new(client)))
        }
        #[cfg(feature = "msgraph")]
        SourceAccountBackend::Msgraph(msgraph) => {
            let client = GraphClient::connect(
                msgraph.kind,
                &msgraph.token,
                &msgraph.user_id,
                msgraph.tls.clone(),
                account.throttle.clone(),
            )?;
            Ok(Client::Msgraph(Box::new(client)))
        }
        #[cfg(feature = "gpeople")]
        SourceAccountBackend::Gpeople(google) => {
            let client = GpeopleClient::connect(
                &google.token,
                google.tls.clone(),
                account.throttle.clone(),
            )?;
            Ok(Client::Gpeople(Box::new(client)))
        }
        #[cfg(feature = "gcal")]
        SourceAccountBackend::Gcal(google) => {
            let client =
                GcalClient::connect(&google.token, google.tls.clone(), account.throttle.clone())?;
            Ok(Client::Gcal(Box::new(client)))
        }
        #[cfg(feature = "dav")]
        SourceAccountBackend::Dav(dav) => {
            let client = DavClient::connect(
                dav.kind,
                &dav.server,
                &dav.tls,
                dav.auth.clone(),
                account.throttle.clone(),
            )?;
            Ok(Client::Dav(Box::new(client)))
        }
        #[cfg(feature = "gmail")]
        SourceAccountBackend::Gmail(gmail) => {
            let client = GmailClient::connect(
                &gmail.token,
                &gmail.user_id,
                gmail.tls.clone(),
                account.throttle.clone(),
            )?;
            Ok(Client::Gmail(Box::new(client)))
        }
        #[cfg(not(any(
            feature = "imap",
            feature = "msgraph",
            feature = "dav",
            feature = "gpeople",
            feature = "gcal",
            feature = "gmail"
        )))]
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

    /// Counts `bytes` received from the endpoint toward the run's report.
    pub fn downloaded(&self, bytes: u64) {
        self.account
            .downloaded
            .fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
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

/// The path segment a new DAV collection named `name` is keyed by: its
/// letters, digits, `-`, `_` and `.` kept, every other run of characters
/// one `-`, and a number appended while a listed collection holds it.
#[cfg(feature = "dav")]
fn free_segment(name: &str, listed: &[Collection]) -> String {
    let mut base = String::new();
    for c in name.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => base.push(c),
            _ if !base.ends_with('-') => base.push('-'),
            _ => {}
        }
    }
    let base = match base.trim_matches(|c| c == '-' || c == '.') {
        "" => "collection",
        trimmed => trimmed,
    }
    .to_owned();

    let taken = |id: &str| listed.iter().any(|collection| collection.id == id);
    let mut id = base.clone();
    let mut n = 2;
    while taken(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

#[cfg(all(test, feature = "dav"))]
mod tests {
    use super::*;

    fn listed(ids: &[&str]) -> Vec<Collection> {
        ids.iter()
            .map(|id| Collection {
                id: id.to_string(),
                name: id.to_string(),
                total: None,
                unread: None,
                role: None,
            })
            .collect()
    }

    #[test]
    fn a_new_dav_collection_is_keyed_by_a_free_segment_of_its_name() {
        assert_eq!(free_segment("Work", &listed(&[])), "Work");
        assert_eq!(
            free_segment("Mes réunions / 2026", &listed(&[])),
            "Mes-r-unions-2026"
        );
        assert_eq!(free_segment("../", &listed(&[])), "collection");
        assert_eq!(free_segment("Work", &listed(&["Work", "Work-2"])), "Work-3");
    }
}
