//! # Graph mail listing
//!
//! How a Graph mail folder is listed (pimdir SYNC §4, §5).
//!
//! Graph's message delta takes no date ceiling, and a link made under a
//! `$filter` reports its filter's changes alone. The checkpoint is therefore
//! a whole-folder delta link whatever the scope, and the connector is bound
//! to no scope ([`Client::scope_bound`](crate::client::Client::scope_bound)).
//!
//! With no scope, a round is the delta itself with the summary `$select`, a
//! page per request, its delta link the checkpoint.
//!
//! Under a scope, a round lists the band: `/messages` filtered on
//! `sentDateTime` (Exchange's submission, which may lie weeks after the
//! `Date` the store scopes by), newest first, each page resuming below the
//! second the last one ended at, never at an offset a delete would shift.
//!
//! The band listed, one delta pass with ids, dates and flags makes the link
//! as the round's last page. It states gone only rows Graph marks removed,
//! absence being the engine's to find; every page states it narrowed by
//! `sentDateTime`, which lists no undated message.
//!
//! A delta follows the stored link: a change outside the scope is dropped
//! by `sentDateTime`; a changed member in scope the store does not bind is
//! named by its summary, batched when the link carries ids only.
//!
//! A run resuming an open round first lists its band head: what Graph
//! received from two days before the round began, newest first, so arrivals
//! show at once. The round's last page lists again what that head would.
//!
//! The link is the same whatever scope a later run asks for: a narrower one
//! keeps it, a wider one lists its band and keeps it. The checkpoint tags
//! whether rows carry summaries or ids; an untagged link under a bounded
//! coverage was made under a `$filter` and is refused, opening a round.

use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDateTime, SecondsFormat, TimeDelta};
use io_msgraph::v1::{
    client::{MsgraphClientStd, MsgraphClientStdError},
    rest::batch::MSGRAPH_BATCH_MAX_REQUESTS,
    rest::users::messages::{
        MsgraphFlagStatus, MsgraphMessage,
        delta::{MsgraphMessagesDeltaParams, MsgraphMessagesDeltaResponse},
        list::{MsgraphMessagesListParams, MsgraphMessagesListResponse},
    },
    send::{MsgraphSend, user_path},
};
use io_pimdir::{
    collection::PimdirScope,
    remote::{PimdirEnumerate, PimdirHead, PimdirListing},
};
use log::{debug, trace, warn};
use url::Url;

use super::{
    GraphClient, SIZE_EXPAND, batch_url, is_expired_link, message_date, message_flags,
    message_meta, message_size,
};
use crate::{
    client::{EnumEntry, Enumeration, Held, Listed, floor_of},
    flag::{Flag, IanaFlag},
};

/// The summary `$select`: what names a message (STORAGE A.1) and its flags.
///
/// `internetMessageHeaders` is the raw read a member is named from, and
/// Graph answers it whole or not at all, trace fields included.
pub(super) const SUMMARY_SELECT: &str = "id,subject,from,toRecipients,ccRecipients,sentDateTime,receivedDateTime,isRead,isDraft,flag,hasAttachments,internetMessageId,internetMessageHeaders,conversationId,parentFolderId";

/// The `$select` of a link made under a scope: id, date and flags.
///
/// Its first pass over a whole folder stays small.
pub(super) const IDS_SELECT: &str = "id,sentDateTime,isRead,isDraft,flag";

/// The messages a page holds at most: `odata.maxpagesize` or `$top`.
///
/// Graph caps message delta at 512. A delta page of a thousand answered in
/// under three seconds on a test tenant (2026-10-07), before
/// [`SUMMARY_SELECT`] named header fields; not measured since.
pub(super) const PAGE_SIZE: u32 = 1000;

/// The messages a page of a listing by arrival holds at most (`$top`): a
/// head lists the few days a round takes, which a page or two holds.
pub(super) const HEAD_PAGE_SIZE: u32 = 512;

/// The margin a band head takes below the instant the round opened (pimdir
/// SYNC §4), for the skew between the store's clock and Graph's.
const HEAD_MARGIN_DAYS: i64 = 2;

/// What a mail listing asks of Graph: the seam [`GraphClient`] serves and
/// the tests fake.
pub(super) trait MailWire {
    /// The first page of the messages of `mailbox` whose `sentDateTime` lies
    /// in `band`, newest first, each with its summary.
    fn band_first(
        &mut self,
        mailbox: &str,
        band: &PimdirScope,
    ) -> Result<MsgraphMessagesListResponse>;

    /// The first page of what `mailbox` received from `from` on, newest first.
    ///
    /// Each with its summary: a band head, and what a round's last page lists
    /// again of it.
    fn received_first(&mut self, mailbox: &str, from: &str) -> Result<MsgraphMessagesListResponse>;

    /// The page a band listing's or a listing by arrival's next link
    /// answers, `None` when Graph no longer serves it.
    fn band_next(&mut self, link: &str) -> Result<Option<MsgraphMessagesListResponse>>;

    /// The first page of a fresh delta of `mailbox` over the whole folder,
    /// with `select`.
    fn delta_first(&mut self, mailbox: &str, select: &str) -> Result<MsgraphMessagesDeltaResponse>;

    /// The page a delta's next or delta link answers, `None` when it expired
    /// (HTTP 410).
    fn delta_next(&mut self, link: &str) -> Result<Option<MsgraphMessagesDeltaResponse>>;

    /// The summaries of `ids`, a message gone since it was listed left out.
    fn metas(&mut self, ids: &[&str]) -> Result<Vec<MsgraphMessage>>;
}

/// A stored delta link and what its rows carry.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Link {
    /// Made by a whole-folder round: every row carries its summary.
    Summary(String),
    /// Made under a scope: a row carries its id, date and flags.
    Ids(String),
}

impl Link {
    /// The checkpoint bytes: the link after a tag naming its kind.
    fn encode(&self) -> Vec<u8> {
        match self {
            Self::Summary(link) => format!("summary {link}").into_bytes(),
            Self::Ids(link) => format!("ids {link}").into_bytes(),
        }
    }

    /// Reads checkpoint bytes back.
    ///
    /// A bare link, stored before links were tagged, was made over the whole
    /// folder unless the coverage is bounded: then a `$filter` made it, and it
    /// is refused.
    fn decode(bytes: &[u8], coverage: Option<&PimdirScope>) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        if let Some(link) = text.strip_prefix("summary ") {
            return Some(Self::Summary(link.to_owned()));
        }
        if let Some(link) = text.strip_prefix("ids ") {
            return Some(Self::Ids(link.to_owned()));
        }
        if text.is_empty() || coverage.is_some_and(|scope| !scope.is_unbounded()) {
            return None;
        }
        Some(Self::Summary(text.to_owned()))
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Summary(link) | Self::Ids(link) => link,
        }
    }

    /// The same kind of link, at `link`.
    fn moved(&self, link: String) -> Self {
        match self {
            Self::Summary(_) => Self::Summary(link),
            Self::Ids(_) => Self::Ids(link),
        }
    }
}

/// Where a round resumes.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Step {
    /// The next page of a whole-folder delta round.
    Summary(String),
    /// The next page of a band listing: what was sent before this instant.
    ///
    /// Never an offset into the band, which a delete between two runs would
    /// shift.
    Below(String),
    /// The band listed, the pass over the folder that makes the link.
    Pass,
}

impl Step {
    fn encode(&self) -> Vec<u8> {
        match self {
            Self::Summary(link) => format!("summary {link}").into_bytes(),
            Self::Below(ceiling) => format!("below {ceiling}").into_bytes(),
            Self::Pass => b"pass".to_vec(),
        }
    }

    /// Reads cursor bytes back.
    ///
    /// A bare link is a delta round's next link, refused when `scope` is
    /// bounded (a `$filter` made it); an untagged band link is an offset into
    /// the band, refused too: the round opens again.
    fn decode(bytes: &[u8], scope: &PimdirScope) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        if let Some(link) = text.strip_prefix("summary ") {
            return Some(Self::Summary(link.to_owned()));
        }
        if let Some(ceiling) = text.strip_prefix("below ") {
            return instant(ceiling).map(Self::Below);
        }
        if text == "pass" {
            return Some(Self::Pass);
        }
        if text.is_empty() || text.starts_with("band ") || !scope.is_unbounded() {
            return None;
        }
        Some(Self::Summary(text.to_owned()))
    }
}

/// What a run keeps of one folder between the pages of its listing.
#[derive(Debug, Default)]
pub(super) struct Folder {
    /// The marks of each message a band round listed this run.
    ///
    /// The pass closing the round lists again only those whose marks moved
    /// (pimdir SYNC §4), then drops them: ids and three bits, no summaries.
    banded: HashMap<String, Marks>,
    /// The instant the open round's band head listed from this run.
    ///
    /// The round's last page lists from it again (pimdir SYNC §4). The engine
    /// lists the head first when resuming a round; one opening it lists none.
    head: Option<String>,
    /// The size Graph stated for each message the last page named, for
    /// the session to plan its body batches by.
    pub(super) sizes: Vec<(String, u64)>,
}

/// The flags a Graph message maps to, in three bits: what a member the
/// store binds is listed by.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Marks(u8);

impl Marks {
    const SEEN: u8 = 1;
    const FLAGGED: u8 = 2;
    const DRAFT: u8 = 4;

    /// The marks of a delta row or a summary.
    pub(super) fn of(message: &MsgraphMessage) -> Self {
        let mut marks = 0;
        if message.is_read == Some(true) {
            marks |= Self::SEEN;
        }
        let status = message.flag.as_ref().and_then(|flag| flag.flag_status);
        if status == Some(MsgraphFlagStatus::Flagged) {
            marks |= Self::FLAGGED;
        }
        if message.is_draft == Some(true) {
            marks |= Self::DRAFT;
        }
        Self(marks)
    }

    /// The shared flag set. `\Answered` and `\Deleted` have no Graph
    /// equivalent and are never produced.
    pub(super) fn flags(self) -> BTreeSet<Flag> {
        [
            (Self::SEEN, IanaFlag::Seen),
            (Self::FLAGGED, IanaFlag::Flagged),
            (Self::DRAFT, IanaFlag::Draft),
        ]
        .into_iter()
        .filter(|(bit, _)| self.0 & bit != 0)
        .map(|(_, flag)| Flag::from_iana(flag))
        .collect()
    }
}

/// Lists one page of the mail folder `mailbox` (pimdir SYNC §4).
///
/// `folder` is what the run keeps of it between pages. A band is listed
/// from its floor up (`sentDateTime` is no `Date`); no delta head is listed,
/// Graph handing its link with a round's last page only.
pub(super) fn enumerate(
    wire: &mut impl MailWire,
    folder: &mut Folder,
    mailbox: &str,
    request: &PimdirEnumerate,
    held: Held<'_>,
) -> Result<Listed> {
    let scope = &request.scope;
    let listed = &floor_of(scope);

    match &request.listing {
        PimdirListing::Head(PimdirHead::Delta(_)) => Ok(Listed::NoHead),
        PimdirListing::Head(PimdirHead::Band { since }) => {
            head(wire, folder, mailbox, since, listed, held)
        }
        PimdirListing::Delta(checkpoint) => match Link::decode(&checkpoint.0, held.coverage) {
            Some(link) => delta(wire, folder, mailbox, &link, listed, held),
            None => {
                warn!("graph delta link of {mailbox} reads as none, opening a round");
                Ok(Listed::CursorRejected)
            }
        },
        PimdirListing::Round { cursor: None, band } => {
            folder.head = None;
            if *band || !scope.is_unbounded() {
                band_page(wire, folder, mailbox, None, *band, listed, held)
            } else {
                summary_page(wire, folder, mailbox, None, held)
            }
        }
        PimdirListing::Round {
            cursor: Some(cursor),
            band,
        } => match Step::decode(&cursor.0, scope) {
            Some(Step::Summary(next)) if scope.is_unbounded() => {
                summary_page(wire, folder, mailbox, Some(&next), held)
            }
            Some(Step::Below(ceiling)) => {
                band_page(wire, folder, mailbox, Some(&ceiling), *band, listed, held)
            }
            Some(Step::Pass) if !band => pass(wire, folder, mailbox, listed, held),
            _ => Ok(Listed::CursorRejected),
        },
    }
}

/// One page of a whole-folder round: the delta with summaries.
///
/// The last page carries the delta link, and lists again what the round's
/// band head would list, read after the link.
fn summary_page(
    wire: &mut impl MailWire,
    folder: &mut Folder,
    mailbox: &str,
    next: Option<&str>,
    held: Held<'_>,
) -> Result<Listed> {
    let page = match next {
        None => wire.delta_first(mailbox, SUMMARY_SELECT)?,
        Some(next) => match wire.delta_next(next)? {
            Some(page) => page,
            None => return Ok(Listed::CursorRejected),
        },
    };

    let (cursor, checkpoint) = match (page.delta_link, page.next_link) {
        (Some(delta), _) => (None, Some(Link::Summary(delta).encode())),
        (None, Some(next)) => (Some(Step::Summary(next).encode()), None),
        (None, None) => bail!("Delta page of {mailbox} carries no paging link"),
    };

    let mut listing = Listing::default();
    for row in latest(
        page.value
            .into_iter()
            .map(|row| (row.message, row.removed.is_some())),
    ) {
        match row {
            (message, true) => listing.vanish(folder, message.id),
            (message, false) => listing.named(folder, message),
        }
    }
    trace!(
        "graph delta page of {mailbox}: {} rows",
        listing.items.len()
    );
    if checkpoint.is_some() {
        let scope = PimdirScope::unbounded();
        relist_head(wire, folder, mailbox, &scope, held, &mut listing)?;
    }

    Ok(Listed::Page(listing.page(true, cursor, checkpoint)))
}

/// One page of a band listing: `scope`'s messages by `sentDateTime`.
///
/// Every page states the round narrowed (undated mail is not listed); a
/// band round's last page hands every id ([`folder_ids`]). A page resumes
/// `below`, the last second included, so a delete shifts no other out.
fn band_page(
    wire: &mut impl MailWire,
    folder: &mut Folder,
    mailbox: &str,
    below: Option<&str>,
    band: bool,
    scope: &PimdirScope,
    held: Held<'_>,
) -> Result<Listed> {
    let listed = PimdirScope {
        since: scope.since.clone(),
        until: below.map(str::to_owned).or_else(|| scope.until.clone()),
    };
    let page = wire.band_first(mailbox, &listed)?;
    let mut messages = page.value;
    let mut next = page.next_link;

    while let Some(link) = next.take_if(|_| within_one_second(&messages)) {
        let page = wire
            .band_next(&link)?
            .with_context(|| format!("Band listing of {mailbox} expired"))?;
        messages.extend(page.value);
        next = page.next_link;
    }

    let cursor = match (next, band) {
        (Some(_), _) => {
            let ceiling = resume_below(&messages)
                .with_context(|| format!("Band page of {mailbox} ends on no date"))?;
            Some(Step::Below(ceiling))
        }
        (None, true) => None,
        (None, false) => Some(Step::Pass),
    };
    let last = cursor.is_none();

    let mut listing = Listing::default();
    for message in messages {
        if in_scope(scope, &message) {
            if !band {
                folder
                    .banded
                    .insert(message.id.clone(), Marks::of(&message));
            }
            listing.named(folder, message);
        }
    }
    trace!(
        "graph band page of {mailbox}: {} messages",
        listing.items.len()
    );
    if last {
        relist_head(wire, folder, mailbox, scope, held, &mut listing)?;
    }
    if last && band {
        listing.enumerated = Some(folder_ids(wire, mailbox, &mut listing.bytes)?);
    }
    listing.narrowed = true;

    Ok(Listed::Page(listing.page(
        true,
        cursor.map(|step| step.encode()),
        None,
    )))
}

/// Every message id of `mailbox`, read by a fresh unfiltered delta.
///
/// A band round's enumeration (pimdir SYNC §4), its link dropped: a fresh
/// delta states no removal, so every row is a member. Octets go to `bytes`.
fn folder_ids(wire: &mut impl MailWire, mailbox: &str, bytes: &mut u64) -> Result<Vec<String>> {
    let followed = follow(wire, mailbox, None, |_, removed| !removed)?
        .with_context(|| format!("Enumeration of {mailbox} expired"))?;
    *bytes += followed.bytes;
    Ok(followed
        .rows
        .into_iter()
        .filter_map(|(id, present)| present.then_some(id))
        .collect())
}

/// Whether `messages` were sent within one second, the first and the last
/// alike: a page the next would list again whole.
fn within_one_second(messages: &[MsgraphMessage]) -> bool {
    let second = |message: Option<&MsgraphMessage>| message_date(message?).map(|at| at.timestamp());
    second(messages.first()) == second(messages.last())
}

/// The instant the page after `messages` lists below, exclusive.
///
/// The second after the last dated message's, so the next page lists that
/// second again rather than leave out one sent then.
fn resume_below(messages: &[MsgraphMessage]) -> Option<String> {
    let at = messages.iter().rev().find_map(message_date)?;
    let below = DateTime::from_timestamp(at.timestamp().checked_add(1)?, 0)?;
    Some(below.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// The open round's band head (pimdir SYNC §4).
///
/// What `scope` received from two days before `since`, the round's opening,
/// with no checkpoint nor removal; the last page lists it again. A `since`
/// reading as no instant lists no head, and the last page owes nothing.
fn head(
    wire: &mut impl MailWire,
    folder: &mut Folder,
    mailbox: &str,
    since: &str,
    scope: &PimdirScope,
    held: Held<'_>,
) -> Result<Listed> {
    let Some(from) = received_floor(since) else {
        warn!("round of {mailbox} opened at {since:?}, which reads as no instant: no head");
        return Ok(Listed::NoHead);
    };
    debug!("list the head of the graph folder {mailbox}, received from {from}");

    let mut listing = Listing::default();
    received(
        wire,
        folder,
        mailbox,
        &from,
        scope,
        held,
        &mut listing,
        HashSet::new(),
    )?;
    folder.head = Some(from);
    trace!("graph head of {mailbox}: {} messages", listing.items.len());

    Ok(Listed::Page(listing.page(false, None, None)))
}

/// What a round's last page owes its band head (pimdir SYNC §4).
///
/// Every member of `scope` that head would list now, unless the page lists
/// or removes it already. Nothing when no head was listed this run.
fn relist_head(
    wire: &mut impl MailWire,
    folder: &mut Folder,
    mailbox: &str,
    scope: &PimdirScope,
    held: Held<'_>,
    listing: &mut Listing,
) -> Result<()> {
    let Some(from) = folder.head.take() else {
        return Ok(());
    };
    debug!("list the head of the graph folder {mailbox} again, received from {from}");

    let listed = listing
        .items
        .iter()
        .map(|entry| entry.id.clone())
        .chain(listing.vanished.iter().cloned())
        .collect();
    received(wire, folder, mailbox, &from, scope, held, listing, listed)
}

/// Lists into `listing` what `scope` received from `from` on.
///
/// Each page read down as it comes, an id in `listed` once: a member the
/// store binds by its flags, any other by its summary.
#[allow(clippy::too_many_arguments)]
fn received(
    wire: &mut impl MailWire,
    folder: &mut Folder,
    mailbox: &str,
    from: &str,
    scope: &PimdirScope,
    held: Held<'_>,
    listing: &mut Listing,
    mut listed: HashSet<String>,
) -> Result<()> {
    let mut page = wire.received_first(mailbox, from)?;
    loop {
        for message in page.value {
            if message.id.is_empty()
                || !in_scope(scope, &message)
                || !listed.insert(message.id.clone())
            {
                continue;
            }
            if held.holds(&message.id) {
                listing.bare(&message.id, Marks::of(&message));
            } else {
                listing.named(folder, message);
            }
        }
        let Some(next) = page.next_link else {
            return Ok(());
        };
        page = wire
            .band_next(&next)?
            .with_context(|| format!("Listing of {mailbox} by arrival expired"))?;
    }
}

/// The instant a band head lists from: `since` less the margin.
///
/// `since` as the store stamped it (RFC 3339 or SQLite's plain form, UTC
/// when it states no offset); the result in UTC to the second for `$filter`.
fn received_floor(since: &str) -> Option<String> {
    let since = since.trim();
    let at = DateTime::parse_from_rfc3339(since)
        .map(|at| at.to_utc())
        .ok()
        .or_else(|| {
            ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"]
                .into_iter()
                .find_map(|format| NaiveDateTime::parse_from_str(since, format).ok())
                .map(|at| at.and_utc())
        })?;
    let from = at.checked_sub_signed(TimeDelta::days(HEAD_MARGIN_DAYS))?;
    Some(from.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// What the pass keeps of the last row of a message: ids, dates and flags
/// read down to what the round's last page needs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Seen {
    /// Removed from the folder.
    Removed,
    /// Present, out of the scope.
    Out,
    /// Present in the scope, with its marks.
    In(Marks),
}

/// The round's last page under a scope: a fresh whole-folder delta pass.
///
/// Its link becomes the checkpoint; only rows it marks removed are gone
/// (SYNC §4). One engine request: resumed, it starts again. A member the
/// band listed this run with unmoved marks is left out; rows read to [`Seen`].
fn pass(
    wire: &mut impl MailWire,
    folder: &mut Folder,
    mailbox: &str,
    scope: &PimdirScope,
    held: Held<'_>,
) -> Result<Listed> {
    debug!("pass over the graph folder {mailbox} for its delta link");

    let Followed {
        rows: listed,
        link: delta,
        bytes,
    } = follow(wire, mailbox, None, |message, removed| match removed {
        true => Seen::Removed,
        false if in_scope(scope, &message) => Seen::In(Marks::of(&message)),
        false => Seen::Out,
    })?
    .with_context(|| format!("Delta pass of {mailbox} expired"))?;

    let mut listing = Listing {
        bytes,
        ..Default::default()
    };
    let mut present = HashSet::new();
    let mut unread = Vec::new();
    let mut gone = Vec::new();
    for (id, seen) in &listed {
        let marks = match seen {
            Seen::Removed => {
                gone.push(id.clone());
                continue;
            }
            Seen::Out => {
                present.insert(id.as_str());
                continue;
            }
            Seen::In(marks) => *marks,
        };
        present.insert(id.as_str());
        if held.holds(id) {
            listing.bare(id, marks);
        } else if folder.banded.get(id) != Some(&marks) {
            unread.push(id.clone());
        }
    }

    folder.banded.clear();
    folder.head = None;
    listing.read_metas(wire, folder, &unread, scope)?;
    for id in gone {
        listing.vanish(folder, id);
    }
    listing.narrowed = true;
    listing.enumerated = Some(present.into_iter().map(String::from).collect());

    Ok(Listed::Page(listing.page(
        true,
        None,
        Some(Link::Ids(delta).encode()),
    )))
}

/// What a delta keeps of the last row of a message: no more than the page
/// it lands needs.
#[derive(Debug)]
enum Change {
    /// Removed from the folder, whatever the scope.
    Removed,
    /// Changed out of the scope: dropped.
    Out,
    /// A member the store binds, listed by its marks.
    Bound(Marks),
    /// A member the store does not bind, named by its row's summary.
    Named(Box<Named>),
    /// A member the store does not bind, whose summary is to read.
    Unread,
}

/// What changed since `link`.
///
/// Removals vanish whatever the scope, changes outside it are dropped, a
/// bound member is listed by its flags, any other by its summary, the
/// row's own or one read in batches.
fn delta(
    wire: &mut impl MailWire,
    folder: &mut Folder,
    mailbox: &str,
    link: &Link,
    scope: &PimdirScope,
    held: Held<'_>,
) -> Result<Listed> {
    let followed = follow(wire, mailbox, Some(link.as_str()), |message, removed| {
        if removed {
            Change::Removed
        } else if !in_scope(scope, &message) {
            Change::Out
        } else if held.holds(&message.id) {
            Change::Bound(Marks::of(&message))
        } else if let Link::Summary(_) = link {
            Change::Named(Box::new(Named::of(message)))
        } else {
            Change::Unread
        }
    })?;
    let Some(Followed {
        rows: listed,
        link: next,
        bytes,
    }) = followed
    else {
        warn!("graph delta link of {mailbox} expired, opening a round");
        return Ok(Listed::CursorRejected);
    };

    let mut listing = Listing {
        bytes,
        ..Default::default()
    };
    let mut unread = Vec::new();
    for (id, change) in listed {
        match change {
            Change::Removed => listing.vanish(folder, id),
            Change::Out => trace!("change of {id} out of scope, dropped"),
            Change::Bound(marks) => listing.bare(&id, marks),
            Change::Named(named) => listing.list(folder, *named),
            Change::Unread => unread.push(id),
        }
    }

    listing.read_metas(wire, folder, &unread, scope)?;

    Ok(Listed::Page(listing.page(
        false,
        None,
        Some(link.moved(next).encode()),
    )))
}

/// Follows a delta from `link`, or a fresh pass with ids only, to its end.
///
/// The last row of each message is read down by `keep` as its page comes,
/// so a folder of any size costs an id and what `keep` makes a message.
/// `None` when Graph answers that the link expired.
fn follow<T>(
    wire: &mut impl MailWire,
    mailbox: &str,
    link: Option<&str>,
    mut keep: impl FnMut(MsgraphMessage, bool) -> T,
) -> Result<Option<Followed<T>>> {
    let mut page = match link {
        None => wire.delta_first(mailbox, IDS_SELECT)?,
        Some(link) => match wire.delta_next(link)? {
            Some(page) => page,
            None => return Ok(None),
        },
    };

    let mut kept: HashMap<String, (usize, T)> = HashMap::new();
    let mut order = 0;
    let mut bytes = 0;
    loop {
        for row in page.value {
            if row.message.id.is_empty() {
                continue;
            }
            bytes += serde_json::to_vec(&row).map_or(0, |json| json.len() as u64);
            let id = row.message.id.clone();
            kept.insert(id, (order, keep(row.message, row.removed.is_some())));
            order += 1;
        }
        if let Some(delta) = page.delta_link {
            let mut listed: Vec<(String, (usize, T))> = kept.into_iter().collect();
            listed.sort_unstable_by_key(|(_, (order, _))| *order);
            let listed = listed.into_iter().map(|(id, (_, kept))| (id, kept));
            return Ok(Some(Followed {
                rows: listed.collect(),
                link: delta,
                bytes,
            }));
        }
        let next = page
            .next_link
            .with_context(|| format!("Delta page of {mailbox} carries no paging link"))?;
        page = match wire.delta_next(&next)? {
            Some(page) => page,
            None => return Ok(None),
        };
    }
}

/// What [`follow`] read of a delta.
///
/// The last row of each message as its caller keeps it, in the order of
/// those rows, the delta link, and the octets the rows took.
struct Followed<T> {
    rows: Vec<(String, T)>,
    link: String,
    bytes: u64,
}

/// A delta row: the message, and whether it was removed.
type Row = (MsgraphMessage, bool);

/// The last row of each id, in the order the ids came: a delta may list a
/// message twice, its later row the current one.
fn latest(rows: impl IntoIterator<Item = Row>) -> Vec<Row> {
    let mut seen = HashSet::new();
    let mut rows: Vec<_> = rows.into_iter().collect();
    rows.reverse();
    rows.retain(|(message, _)| !message.id.is_empty() && seen.insert(message.id.clone()));
    rows.reverse();
    rows
}

/// Whether a message's `sentDateTime` lies in `scope`, no date being in
/// every scope.
fn in_scope(scope: &PimdirScope, message: &MsgraphMessage) -> bool {
    let date =
        message_date(message).map(|date| date.to_utc().to_rfc3339_opts(SecondsFormat::Secs, true));
    scope.contains(date.as_deref())
}

/// An RFC 3339 instant as the scope compares it: UTC, to the second, `Z`.
fn instant(text: &str) -> Option<String> {
    let date = DateTime::parse_from_rfc3339(text).ok()?;
    Some(date.to_utc().to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// The `$filter` of a band listing: `sentDateTime` in the scope.
///
/// Floor inclusive, ceiling (where a page resumes) exclusive. A bound that
/// does not read as an instant is left out, a superset the local check
/// narrows.
pub(super) fn band_filter(scope: &PimdirScope) -> Option<String> {
    let since = scope.since.as_deref().and_then(instant);
    let until = scope.until.as_deref().and_then(instant);
    let terms: Vec<String> = since
        .map(|since| format!("sentDateTime ge {since}"))
        .into_iter()
        .chain(until.map(|until| format!("sentDateTime lt {until}")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" and "))
}

/// A page being built: its entries, its removals, its meta octets, and
/// what it states of a date filter (pimdir SYNC §4, §5).
#[derive(Default)]
struct Listing {
    items: Vec<EnumEntry>,
    vanished: Vec<String>,
    bytes: u64,
    narrowed: bool,
    enumerated: Option<Vec<String>>,
}

impl Listing {
    /// Lists `message` named by its summary, noting the size Graph states.
    fn named(&mut self, folder: &mut Folder, message: MsgraphMessage) {
        self.list(folder, Named::of(message));
    }

    /// Lists a member its row named, noting the size Graph states.
    fn list(&mut self, folder: &mut Folder, named: Named) {
        self.bytes += named.bytes;
        if let Some(size) = named.size {
            folder.sizes.push((named.entry.id.clone(), size));
        }
        self.items.push(named.entry);
    }

    /// Lists a member the store binds by its flags, the store naming it.
    fn bare(&mut self, id: &str, marks: Marks) {
        self.items.push(EnumEntry {
            id: id.to_owned(),
            flags: marks.flags(),
            revision: None,
            meta: None,
        });
    }

    /// States `id` removed.
    fn vanish(&mut self, folder: &mut Folder, id: String) {
        folder.banded.remove(&id);
        self.vanished.push(id);
    }

    /// Reads the summaries of `ids` and lists those in scope.
    ///
    /// A message gone since is left out; any other one Graph did not answer
    /// fails the listing, which the next run lists again.
    fn read_metas(
        &mut self,
        wire: &mut impl MailWire,
        folder: &mut Folder,
        ids: &[String],
        scope: &PimdirScope,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        debug!("read the summaries of {} unbound messages", ids.len());

        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        for message in wire.metas(&ids)? {
            if in_scope(scope, &message) {
                self.named(folder, message);
            }
        }
        Ok(())
    }

    fn page(
        self,
        complete: bool,
        cursor: Option<Vec<u8>>,
        checkpoint: Option<Vec<u8>>,
    ) -> Enumeration {
        Enumeration {
            items: self.items,
            vanished: self.vanished,
            complete,
            cursor,
            checkpoint,
            bytes: self.bytes,
            narrowed: self.narrowed,
            enumerated: self.enumerated,
            truncated: false,
            errored: Vec::new(),
        }
    }
}

/// A member its row names, the row and its header fields dropped.
#[derive(Debug)]
struct Named {
    entry: EnumEntry,
    /// The size Graph states for the message.
    size: Option<u64>,
    /// The octets the row took.
    bytes: u64,
}

impl Named {
    fn of(message: MsgraphMessage) -> Self {
        Self {
            size: message_size(&message).filter(|size| *size > 0),
            bytes: serde_json::to_vec(&message).map_or(0, |json| json.len() as u64),
            entry: EnumEntry {
                flags: message_flags(&message),
                revision: None,
                meta: Some(message_meta(&message)),
                id: message.id,
            },
        }
    }
}

impl MailWire for GraphClient {
    fn band_first(
        &mut self,
        mailbox: &str,
        band: &PimdirScope,
    ) -> Result<MsgraphMessagesListResponse> {
        let folder = self.folder_id(mailbox)?;
        let filter = band_filter(band);
        let params = MsgraphMessagesListParams {
            top: Some(PAGE_SIZE),
            select: Some(SUMMARY_SELECT),
            filter: filter.as_deref(),
            orderby: Some("sentDateTime desc"),
            expand: Some(SIZE_EXPAND),
            ..Default::default()
        };
        self.op(|client| client.messages_list(Some(&folder), &params))
            .with_context(|| format!("List {mailbox} by date error"))
    }

    /// Graph orders by `receivedDateTime` only when the `$filter` names it.
    ///
    /// The size `$expand`, refused once (400), is asked no more by the
    /// session, the sizes then read before the bodies.
    fn received_first(&mut self, mailbox: &str, from: &str) -> Result<MsgraphMessagesListResponse> {
        let folder = self.folder_id(mailbox)?;
        let filter = format!("receivedDateTime ge {from}");
        let params = |expand: bool| MsgraphMessagesListParams {
            top: Some(HEAD_PAGE_SIZE),
            select: Some(SUMMARY_SELECT),
            filter: Some(filter.as_str()),
            orderby: Some("receivedDateTime desc"),
            expand: expand.then_some(SIZE_EXPAND),
            ..Default::default()
        };

        let expands = self.received_expands;
        let first = self.op(|client| client.messages_list(Some(&folder), &params(expands)));
        let first = match first {
            Err(MsgraphClientStdError::Send(err)) if expands && err.status() == Some(400) => {
                warn!(
                    "graph refused the size $expand on the arrivals of {mailbox} ({err}), asking without it"
                );
                self.received_expands = false;
                self.op(|client| client.messages_list(Some(&folder), &params(false)))
            }
            first => first,
        };
        first.with_context(|| format!("List {mailbox} by arrival error"))
    }

    fn band_next(&mut self, link: &str) -> Result<Option<MsgraphMessagesListResponse>> {
        let url = Url::parse(link).context("Cannot parse the listing's next link")?;
        let page = self.op(|client: &mut MsgraphClientStd| {
            let coroutine =
                MsgraphSend::<MsgraphMessagesListResponse>::get(&client.auth, url.clone());
            client.run(coroutine)
        });
        match page {
            Ok(page) => Ok(Some(page)),
            Err(err) if is_expired_link(&err) => Ok(None),
            Err(err) => {
                Err(anyhow::Error::new(err).context("Follow the listing's next link error"))
            }
        }
    }

    /// A whole-folder delta asks for the size `$expand` too.
    ///
    /// Graph documents it for message delta; refused once (400), the session
    /// asks without it from then on, the sizes then read before the bodies.
    fn delta_first(&mut self, mailbox: &str, select: &str) -> Result<MsgraphMessagesDeltaResponse> {
        let folder = self.folder_id(mailbox)?;
        let expands = select == SUMMARY_SELECT && self.delta_expands;
        let params = |expand: bool| MsgraphMessagesDeltaParams {
            select: Some(select),
            filter: None,
            expand: expand.then_some(SIZE_EXPAND),
            max_page_size: Some(PAGE_SIZE),
        };

        let first =
            self.op(|client| client.messages_delta_with_params(Some(&folder), &params(expands)));
        let first = match first {
            Err(MsgraphClientStdError::Send(err)) if expands && err.status() == Some(400) => {
                warn!(
                    "graph refused the size $expand on the delta of {mailbox} ({err}), asking without it"
                );
                self.delta_expands = false;
                self.op(|client| client.messages_delta_with_params(Some(&folder), &params(false)))
            }
            first => first,
        };
        first.with_context(|| format!("Start delta of {mailbox} error"))
    }

    fn delta_next(&mut self, link: &str) -> Result<Option<MsgraphMessagesDeltaResponse>> {
        match self
            .op(|client| client.messages_delta_from_link_with_page_size(link, Some(PAGE_SIZE)))
        {
            Ok(page) => Ok(Some(page)),
            Err(err) if is_expired_link(&err) => Ok(None),
            Err(err) => Err(anyhow::Error::new(err).context("Follow delta link error")),
        }
    }

    fn metas(&mut self, ids: &[&str]) -> Result<Vec<MsgraphMessage>> {
        let user = user_path(&self.inner.user_id)?;
        let mut metas = Vec::with_capacity(ids.len());

        for chunk in ids.chunks(MSGRAPH_BATCH_MAX_REQUESTS) {
            let mut read = HashMap::new();
            let answer = self
                .batch_get(
                    chunk,
                    |id| {
                        batch_url(
                            &format!("{user}/messages/{id}"),
                            &[("$select", SUMMARY_SELECT), ("$expand", SIZE_EXPAND)],
                        )
                    },
                    |id, body| {
                        let message = body
                            .and_then(|body| serde_json::from_value::<MsgraphMessage>(body).ok());
                        if message.is_none() {
                            warn!("graph batch summary of {id} does not read as a message");
                        }
                        message
                    },
                    |id, message| {
                        read.insert(id, message);
                        Ok(())
                    },
                )
                .context("Send summary batch error")?;

            let missing = chunk
                .iter()
                .filter(|id| !read.contains_key(*id) && !answer.gone.contains(id))
                .count();
            if missing > 0 {
                bail!("Graph did not answer the summary of {missing} changed messages");
            }

            for id in chunk {
                if let Some(mut message) = read.remove(id) {
                    if message.id.is_empty() {
                        message.id = (*id).to_owned();
                    }
                    metas.push(message);
                }
            }
        }

        Ok(metas)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use io_msgraph::v1::rest::users::messages::delta::MsgraphMessageDelta;
    use io_pimdir::{
        change::PimdirChange,
        client::{PimdirSourceStore, PimdirStore},
        collection::{PimdirCollectionId, PimdirCursor},
        load::PimdirLoadSelection,
        placement::{PimdirHandle, PimdirLinkId, PimdirSortKey},
        remote::{
            PimdirEnumerated, PimdirFetchedItem, PimdirHeads, PimdirPushResult, PimdirRemote,
            PimdirRemoteItem, PimdirRemoteMeta, PimdirRemoteSnapshot, PimdirTier,
        },
        sync::{PimdirSync, PimdirSyncOptions},
    };

    use super::*;
    use crate::sync::{
        remote::{Narrowing, keep_in_scope, snapshot},
        run_verb,
        storage::load_side,
    };

    const COLLECTION: &str = "msgraph/INBOX";

    /// A listing reads `sentDateTime` from its floor, inclusive, below its
    /// ceiling, exclusive, in UTC.
    #[test]
    fn a_band_filters_the_sent_date_on_the_scope_bounds() {
        let scope = |since: Option<&str>, until: Option<&str>| PimdirScope {
            since: since.map(String::from),
            until: until.map(String::from),
        };

        assert_eq!(
            band_filter(&scope(Some("2026-09-01T00:00:00Z"), None)).as_deref(),
            Some("sentDateTime ge 2026-09-01T00:00:00Z"),
            "the newest band stays open above",
        );
        assert_eq!(
            band_filter(&scope(
                Some("2026-06-01T00:00:00Z"),
                Some("2026-09-01T00:00:00Z")
            ))
            .as_deref(),
            Some("sentDateTime ge 2026-06-01T00:00:00Z and sentDateTime lt 2026-09-01T00:00:00Z"),
            "a page resuming below a second",
        );
        assert_eq!(
            band_filter(&scope(None, Some("2026-01-01T01:00:00+02:00"))).as_deref(),
            Some("sentDateTime lt 2025-12-31T23:00:00Z"),
        );
        assert_eq!(band_filter(&scope(Some("soon"), None)), None);
        assert_eq!(band_filter(&PimdirScope::unbounded()), None);
    }

    #[test]
    fn a_link_names_its_kind_and_a_bare_one_under_a_bounded_coverage_is_refused() {
        let link = "https://graph.microsoft.com/v1.0/me/mailFolders/x/messages/delta?$deltatoken=a";
        for tagged in [Link::Summary(link.into()), Link::Ids(link.into())] {
            assert_eq!(Link::decode(&tagged.encode(), None), Some(tagged.clone()));
        }

        let bounded = PimdirScope::since("2026-09-01T00:00:00Z");
        assert_eq!(
            Link::decode(link.as_bytes(), Some(&PimdirScope::unbounded())),
            Some(Link::Summary(link.into())),
            "a bare link of a whole-folder round carries summaries",
        );
        assert_eq!(
            Link::decode(link.as_bytes(), Some(&bounded)),
            None,
            "a bare link under a bounded coverage was made under a $filter",
        );
        assert_eq!(Link::decode(b"", None), None);
        assert_eq!(Link::decode(&[0xff, 0xfe], None), None);

        assert_eq!(
            Step::decode(link.as_bytes(), &PimdirScope::unbounded()),
            Some(Step::Summary(link.into()))
        );
        assert_eq!(Step::decode(link.as_bytes(), &bounded), None);
        for step in [
            Step::Below("2026-09-03T10:00:01Z".into()),
            Step::Pass,
            Step::Summary(link.into()),
        ] {
            assert_eq!(Step::decode(&step.encode(), &bounded), Some(step.clone()));
        }
        assert_eq!(
            Step::decode(format!("band {link}").as_bytes(), &bounded),
            None,
            "a band's next link of before is an offset, refused",
        );
    }

    /// One message of the fake folder.
    #[derive(Clone)]
    struct Message {
        sent: Option<String>,
        /// When Graph received it, UTC to the second: long ago for the
        /// folder a test starts with, now for an arrival.
        received: String,
        /// The `Date` header its row states, none when the row states no
        /// header field and is dated by its `sentDateTime`.
        dated: Option<String>,
        read: bool,
        flagged: bool,
    }

    /// When the folder a test starts with was received: before any round
    /// a test opens, so no head lists it.
    const LONG_AGO: &str = "2001-01-01T00:00:00Z";

    /// What the fake answered, by kind of request.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Counts {
        /// Band pages asked by date, each a listing of its own.
        bands: usize,
        /// Band pages served, the first included.
        band_pages: usize,
        /// Messages a band page listed.
        band_rows: usize,
        /// Fresh deltas started, with their select.
        passes: Vec<String>,
        /// Delta pages served from a link.
        delta_pages: usize,
        /// Rows a delta page listed, fresh or followed.
        delta_rows: usize,
        /// Summaries read.
        metas: usize,
        /// Listings by arrival started, with the instant they list from.
        received: Vec<String>,
        /// Messages a listing by arrival listed.
        received_rows: usize,
    }

    /// A Graph mail folder in memory.
    ///
    /// Band listings by `sentDateTime`, newest first, and a whole-folder
    /// delta whose links are the version of its change log, `page` rows a page.
    struct FakeGraph {
        page: usize,
        messages: BTreeMap<String, Message>,
        /// Every change, by id, in order: a link at version `v` reports
        /// the ids changed from `log[v]` on.
        log: Vec<String>,
        /// The versions whose links Graph no longer serves.
        expired: HashSet<usize>,
        /// The messages a fresh delta fails to list while the folder holds
        /// them, as one ending early would.
        unlisted: HashSet<String>,
        counts: Counts,
    }

    impl FakeGraph {
        fn new(page: usize) -> Self {
            Self {
                page,
                messages: BTreeMap::new(),
                log: Vec::new(),
                expired: HashSet::new(),
                unlisted: HashSet::new(),
                counts: Counts::default(),
            }
        }

        fn add(&mut self, id: &str, sent: Option<&str>) {
            self.received(id, sent, LONG_AGO);
        }

        /// A message Graph receives now, as a round goes on.
        fn arrive(&mut self, id: &str, sent: &str) {
            let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
            self.received(id, Some(sent), &now);
        }

        fn received(&mut self, id: &str, sent: Option<&str>, received: &str) {
            self.messages.insert(
                id.to_owned(),
                Message {
                    sent: sent.map(String::from),
                    received: received.to_owned(),
                    dated: None,
                    read: false,
                    flagged: false,
                },
            );
            self.log.push(id.to_owned());
        }

        /// A message Graph states sent at `sent`, its `Date` header `date`:
        /// written long before Exchange submitted it.
        fn sent_after(&mut self, id: &str, sent: &str, date: &str) {
            self.received(id, Some(sent), LONG_AGO);
            self.messages.get_mut(id).unwrap().dated = Some(date.to_owned());
        }

        fn flag(&mut self, id: &str) {
            self.messages.get_mut(id).unwrap().flagged = true;
            self.log.push(id.to_owned());
        }

        fn delete(&mut self, id: &str) {
            self.messages.remove(id);
            self.log.push(id.to_owned());
        }

        /// The row of `id`: its summary, or its id, date and flags.
        fn row(&self, id: &str, summary: bool) -> MsgraphMessage {
            let message = &self.messages[id];
            let mut row = serde_json::json!({
                "id": id,
                "sentDateTime": message.sent,
                "receivedDateTime": message.received,
                "isRead": message.read,
                "isDraft": false,
                "flag": { "flagStatus": if message.flagged { "flagged" } else { "notFlagged" } },
            });
            if summary {
                row["subject"] = serde_json::json!(format!("Subject {id}"));
                row["internetMessageId"] = serde_json::json!(format!("<{id}@example.org>"));
                row["hasAttachments"] = serde_json::json!(false);
                if let Some(date) = &message.dated {
                    row["internetMessageHeaders"] = serde_json::json!([
                        { "name": "Message-ID", "value": format!("<{id}@example.org>") },
                        { "name": "Date", "value": date },
                    ]);
                }
            }
            serde_json::from_value(row).unwrap()
        }

        fn delta_row(&self, id: &str, summary: bool) -> MsgraphMessageDelta {
            let row = match self.messages.contains_key(id) {
                true => serde_json::to_value(self.row(id, summary)).unwrap(),
                false => serde_json::json!({ "id": id, "@removed": { "reason": "deleted" } }),
            };
            serde_json::from_value(row).unwrap()
        }

        /// The ids of `band`, newest first, as Graph's `$filter` and
        /// `$orderby` on `sentDateTime` answer them: no date, no match.
        fn band(&self, band: &PimdirScope) -> Vec<String> {
            let mut ids: Vec<(&String, &String)> = self
                .messages
                .iter()
                .filter_map(|(id, message)| Some((message.sent.as_ref()?, id)))
                .filter(|(sent, _)| band.contains(Some(sent)))
                .collect();
            ids.sort();
            ids.into_iter().rev().map(|(_, id)| id.clone()).collect()
        }

        fn band_page(&mut self, band: PimdirScope, offset: usize) -> MsgraphMessagesListResponse {
            self.counts.band_pages += 1;
            let ids = self.band(&band);
            let end = (offset + self.page).min(ids.len());
            let value: Vec<MsgraphMessage> = ids[offset..end]
                .iter()
                .map(|id| self.row(id, true))
                .collect();
            self.counts.band_rows += value.len();
            let next_link = (end < ids.len()).then(|| {
                format!(
                    "band|{}|{}|{end}",
                    band.since.unwrap_or_default(),
                    band.until.unwrap_or_default()
                )
            });
            MsgraphMessagesListResponse {
                value,
                next_link,
                ..Default::default()
            }
        }

        /// A page of the messages received from `from` on, newest first.
        ///
        /// From `offset`, as Graph's `$filter` and `$orderby` on
        /// `receivedDateTime` answer them.
        fn received_page(&mut self, from: &str, offset: usize) -> MsgraphMessagesListResponse {
            let mut ids: Vec<(&String, &String)> = self
                .messages
                .iter()
                .filter(|(_, message)| message.received.as_str() >= from)
                .map(|(id, message)| (&message.received, id))
                .collect();
            ids.sort();
            let ids: Vec<String> = ids.into_iter().rev().map(|(_, id)| id.clone()).collect();
            let end = (offset + self.page).min(ids.len());
            let value: Vec<MsgraphMessage> = ids[offset..end]
                .iter()
                .map(|id| self.row(id, true))
                .collect();
            self.counts.received_rows += value.len();
            let next_link = (end < ids.len()).then(|| format!("received|{from}|{end}"));
            MsgraphMessagesListResponse {
                value,
                next_link,
                ..Default::default()
            }
        }

        /// A page of `ids` from `offset`, with the link to the next one or,
        /// last, the delta link at the current version.
        fn delta_page(
            &mut self,
            ids: &[String],
            offset: usize,
            summary: bool,
            next: impl Fn(usize) -> String,
        ) -> MsgraphMessagesDeltaResponse {
            let end = (offset + self.page).min(ids.len());
            let value: Vec<MsgraphMessageDelta> = ids[offset..end]
                .iter()
                .map(|id| self.delta_row(id, summary))
                .collect();
            self.counts.delta_rows += value.len();
            let select = if summary { "summary" } else { "ids" };
            let (next_link, delta_link) = match end < ids.len() {
                true => (Some(next(end)), None),
                false => (None, Some(format!("delta|{select}|{}", self.log.len()))),
            };
            MsgraphMessagesDeltaResponse {
                value,
                next_link,
                delta_link,
                count: None,
            }
        }

        /// The ids a fresh delta lists: every message but the unlisted.
        fn passed(&self) -> Vec<String> {
            let listed = |id: &&String| !self.unlisted.contains(*id);
            self.messages.keys().filter(listed).cloned().collect()
        }

        /// The ids changed since `version`, each once, in change order.
        fn changed(&self, version: usize) -> Vec<String> {
            let mut seen = HashSet::new();
            self.log[version.min(self.log.len())..]
                .iter()
                .filter(|id| seen.insert((*id).clone()))
                .cloned()
                .collect()
        }
    }

    impl MailWire for FakeGraph {
        fn band_first(
            &mut self,
            _mailbox: &str,
            band: &PimdirScope,
        ) -> Result<MsgraphMessagesListResponse> {
            self.counts.bands += 1;
            Ok(self.band_page(band.clone(), 0))
        }

        fn received_first(
            &mut self,
            _mailbox: &str,
            from: &str,
        ) -> Result<MsgraphMessagesListResponse> {
            self.counts.received.push(from.to_owned());
            Ok(self.received_page(from, 0))
        }

        fn band_next(&mut self, link: &str) -> Result<Option<MsgraphMessagesListResponse>> {
            let parts: Vec<&str> = link.split('|').collect();
            if parts[0] == "received" {
                return Ok(Some(self.received_page(parts[1], parts[2].parse()?)));
            }
            let bound = |text: &str| (!text.is_empty()).then(|| text.to_owned());
            let band = PimdirScope {
                since: bound(parts[1]),
                until: bound(parts[2]),
            };
            Ok(Some(self.band_page(band, parts[3].parse()?)))
        }

        fn delta_first(
            &mut self,
            _mailbox: &str,
            select: &str,
        ) -> Result<MsgraphMessagesDeltaResponse> {
            self.counts.passes.push(select.to_owned());
            let summary = select == SUMMARY_SELECT;
            let ids = self.passed();
            Ok(self.delta_page(&ids, 0, summary, |end| format!("pass|{summary}|{end}")))
        }

        fn delta_next(&mut self, link: &str) -> Result<Option<MsgraphMessagesDeltaResponse>> {
            self.counts.delta_pages += 1;
            let parts: Vec<&str> = link.split('|').collect();
            match parts[0] {
                "pass" => {
                    let summary = parts[1] == "true";
                    let ids = self.passed();
                    Ok(Some(self.delta_page(
                        &ids,
                        parts[2].parse()?,
                        summary,
                        |end| format!("pass|{summary}|{end}"),
                    )))
                }
                "delta" | "changes" => {
                    let summary = parts[1] == "summary";
                    let version: usize = parts[2].parse()?;
                    if self.expired.contains(&version) {
                        return Ok(None);
                    }
                    let offset: usize = parts.get(3).map_or(Ok(0), |o| o.parse())?;
                    let ids = self.changed(version);
                    let select = parts[1].to_owned();
                    Ok(Some(self.delta_page(&ids, offset, summary, |end| {
                        format!("changes|{select}|{version}|{end}")
                    })))
                }
                other => bail!("unknown fake link {other}"),
            }
        }

        fn metas(&mut self, ids: &[&str]) -> Result<Vec<MsgraphMessage>> {
            self.counts.metas += ids.len();
            Ok(ids
                .iter()
                .filter(|id| self.messages.contains_key(**id))
                .map(|id| self.row(id, true))
                .collect())
        }
    }

    /// A change made to the fake folder between two enumerations.
    type Change = Box<dyn FnOnce(&mut FakeGraph)>;

    /// The fake Graph folder behind the engine's seam.
    ///
    /// It names a member listed bare by its binding as `PimRemote` does, and
    /// fails the enumeration numbered `fail_at`, as a dropped connection would.
    struct Remote {
        graph: FakeGraph,
        folder: Folder,
        handles: HashSet<String>,
        names: HashMap<String, (PimdirLinkId, PimdirSortKey)>,
        checkpoint: Option<Vec<u8>>,
        coverage: Option<PimdirScope>,
        requests: Vec<PimdirEnumerate>,
        fail_at: Option<usize>,
        /// Run between two enumerations, once, before the one numbered by
        /// its key.
        between: Option<(usize, Change)>,
        /// The heads the remote states, none unless a test lists them.
        heads: PimdirHeads,
        /// What each page stated of its date filter: whether it was
        /// narrowed, and how many handles its enumeration held.
        stated: Vec<(bool, Option<usize>)>,
    }

    impl Remote {
        fn new(graph: FakeGraph) -> Self {
            Self {
                graph,
                folder: Folder::default(),
                handles: HashSet::new(),
                names: HashMap::new(),
                checkpoint: None,
                coverage: None,
                requests: Vec::new(),
                fail_at: None,
                between: None,
                heads: PimdirHeads::NONE,
                stated: Vec::new(),
            }
        }
    }

    impl PimdirRemote for Remote {
        type Error = anyhow::Error;

        fn enumerate(
            &mut self,
            _collection: &PimdirCollectionId,
            request: PimdirEnumerate,
        ) -> Result<PimdirEnumerated, Self::Error> {
            let index = self.requests.len();
            self.requests.push(request.clone());
            if self.between.as_ref().is_some_and(|(at, _)| *at == index) {
                let (_, change) = self.between.take().unwrap();
                change(&mut self.graph);
            }
            if self.fail_at == Some(index) {
                self.fail_at = None;
                bail!("connection reset by peer");
            }

            let held = Held {
                handles: &self.handles,
                checkpoint: self.checkpoint.as_deref(),
                coverage: self.coverage.as_ref(),
            };
            let page = match enumerate(&mut self.graph, &mut self.folder, "INBOX", &request, held)?
            {
                Listed::Page(page) => page,
                Listed::CursorRejected => return Ok(PimdirEnumerated::CursorRejected),
                Listed::NoHead => return Ok(PimdirEnumerated::NoHead),
                Listed::HeadRejected => return Ok(PimdirEnumerated::HeadRejected),
                Listed::Rekey => anyhow::bail!("a Graph folder is never rekeyed"),
            };

            let mut items: Vec<PimdirRemoteItem> = page
                .items
                .into_iter()
                .filter_map(|entry| {
                    let meta = match entry.meta {
                        Some(derivation) => PimdirRemoteMeta::from(derivation),
                        None => {
                            let (link_id, sort_key) = self.names.get(&entry.id)?.clone();
                            PimdirRemoteMeta {
                                link_id,
                                summary: None,
                                sort_key,
                                body: None,
                                server_object_id: None,
                            }
                        }
                    };
                    Some(PimdirRemoteItem {
                        handle: PimdirHandle(entry.id),
                        flags: entry.flags.iter().map(|flag| flag.raw()).collect(),
                        revision: None,
                        meta,
                    })
                })
                .collect();
            keep_in_scope(&mut items, &request.scope);
            let vanished = page.vanished.into_iter().map(PimdirHandle).collect();

            if let Some(PimdirHead::Band { .. }) = request.head() {
                return Ok(PimdirEnumerated::Page(PimdirRemoteSnapshot::band_head(
                    items,
                )));
            }
            self.stated
                .push((page.narrowed, page.enumerated.as_ref().map(Vec::len)));
            let narrowing = Narrowing {
                narrowed: page.narrowed,
                enumerated: page.enumerated,
            };
            Ok(PimdirEnumerated::Page(snapshot(
                page.complete,
                page.cursor,
                page.checkpoint,
                items,
                vanished,
                narrowing,
            )))
        }

        fn fetch(
            &mut self,
            _collection: &PimdirCollectionId,
            _handles: Vec<PimdirHandle>,
            _tier: PimdirTier,
        ) -> Result<Vec<PimdirFetchedItem>, Self::Error> {
            bail!("every member arrives named")
        }

        fn push(
            &mut self,
            _collection: &PimdirCollectionId,
            changes: Vec<PimdirChange>,
        ) -> Result<Vec<PimdirPushResult>, Self::Error> {
            bail!("nothing is staged, so nothing pushes: {changes:?}")
        }

        fn scope_bound(&self) -> bool {
            false
        }

        fn heads(&self) -> PimdirHeads {
            self.heads
        }
    }

    fn store(dir: &std::path::Path) -> PimdirSourceStore {
        let store = PimdirStore::open(dir)
            .unwrap()
            .for_source("msgraph")
            .unwrap();
        store
            .ensure_collection(COLLECTION, "message/rfc822")
            .unwrap();
        store
    }

    /// Syncs the folder under `scope` as the driver does, the remote told
    /// what the store binds first.
    fn sync(store: &mut PimdirSourceStore, remote: &mut Remote, scope: PimdirScope) -> Result<()> {
        let loaded = store
            .load(
                &PimdirCollectionId(COLLECTION.into()),
                &PimdirLoadSelection::All,
            )
            .unwrap();
        remote.checkpoint = loaded.checkpoint.map(|checkpoint| checkpoint.0);
        remote.coverage = loaded.coverage.map(|coverage| coverage.scope);
        remote.names = loaded
            .placements
            .into_iter()
            .filter(|placement| placement.base.is_some())
            .filter_map(|placement| {
                Some((placement.handle.0, (placement.link_id?, placement.sort_key)))
            })
            .collect();
        remote.handles = remote.names.keys().cloned().collect();

        let opts = PimdirSyncOptions {
            scope,
            ..Default::default()
        };
        let verb = PimdirSync::new(COLLECTION, opts)
            .beside_other_sources(true)
            .scope_bound(false)
            .heads(remote.heads);
        run_verb(store, remote, verb).map(|_| ())
    }

    fn loaded(store: &PimdirSourceStore) -> io_pimdir::load::PimdirLoaded {
        store
            .load(
                &PimdirCollectionId(COLLECTION.into()),
                &PimdirLoadSelection::Handles(Vec::new()),
            )
            .unwrap()
    }

    fn checkpoint(store: &PimdirSourceStore) -> Option<Vec<u8>> {
        loaded(store).checkpoint.map(|checkpoint| checkpoint.0)
    }

    fn coverage(store: &PimdirSourceStore) -> Option<PimdirScope> {
        loaded(store).coverage.map(|coverage| coverage.scope)
    }

    fn stored(store: &PimdirSourceStore) -> BTreeSet<String> {
        load_side(store, COLLECTION)
            .unwrap()
            .into_iter()
            .map(|placement| placement.handle.0)
            .collect()
    }

    fn flags_of(store: &PimdirSourceStore, id: &str) -> BTreeSet<String> {
        load_side(store, COLLECTION)
            .unwrap()
            .into_iter()
            .find(|placement| placement.handle.0 == id)
            .and_then(|placement| placement.flags.known().cloned())
            .unwrap_or_default()
    }

    fn since(date: &str) -> PimdirScope {
        PimdirScope::since(format!("{date}T00:00:00Z"))
    }

    /// Ten messages a month over four months (`<month>-<n>`), and one with
    /// no date at all, three a page.
    fn folder() -> FakeGraph {
        let mut graph = FakeGraph::new(3);
        for month in ["2026-03", "2026-06", "2026-07", "2026-09"] {
            for day in 1..=10 {
                let id = format!("{month}-{day:02}");
                graph.add(&id, Some(&format!("{month}-{day:02}T10:00:00Z")));
            }
        }
        graph.add("undated", None);
        graph
    }

    fn month(month: &str) -> Vec<String> {
        (1..=10).map(|day| format!("{month}-{day:02}")).collect()
    }

    fn months(list: &[&str]) -> BTreeSet<String> {
        list.iter()
            .flat_map(|m| month(m))
            .chain(["undated".into()])
            .collect()
    }

    /// Three widenings list their band from its floor up, and keep the link.
    ///
    /// The folder is passed over once, by the first round, and a run within
    /// the coverage follows the link.
    #[test]
    fn widenings_list_their_band_and_keep_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut remote = Remote::new(folder());

        sync(&mut store, &mut remote, since("2026-09-01")).unwrap();
        assert_eq!(stored(&store), months(&["2026-09"]));
        let counts = remote.graph.counts.clone();
        assert_eq!(counts.bands, 5, "five pages of three, each resuming below");
        assert_eq!(
            counts.band_rows, 14,
            "the band lists its ten messages, the next page again the one a page ended at"
        );
        assert_eq!(counts.passes, [IDS_SELECT], "one pass, ids only");
        assert_eq!(
            counts.delta_rows, 41,
            "the pass lists the whole folder once"
        );
        assert_eq!(counts.metas, 1, "only the undated message is read");
        let link = checkpoint(&store).expect("the round closed with the link");
        assert!(
            link.starts_with(b"ids "),
            "{}",
            String::from_utf8_lossy(&link)
        );

        for (floor, listed) in [
            ("2026-07-01", &["2026-07", "2026-09"][..]),
            ("2026-06-01", &["2026-06", "2026-07", "2026-09"][..]),
            (
                "2026-03-01",
                &["2026-03", "2026-06", "2026-07", "2026-09"][..],
            ),
        ] {
            let requests = remote.requests.len();
            sync(&mut store, &mut remote, since(floor)).unwrap();
            assert_eq!(stored(&store), months(listed), "widened to {floor}");
            assert!(
                remote.requests[requests..].iter().all(|request| matches!(
                    request.listing,
                    PimdirListing::Round { band: true, .. }
                )),
                "a widening lists its band alone",
            );
            assert_eq!(
                checkpoint(&store),
                Some(link.clone()),
                "the band keeps the link"
            );
            assert_eq!(coverage(&store), Some(since(floor)));
        }

        let counts = remote.graph.counts.clone();
        assert_eq!(counts.bands, 50);
        assert_eq!(
            counts.band_rows, 146,
            "each dated message from the band's floor up, and the one each page ended at again"
        );
        assert_eq!(
            counts.passes.len(),
            4,
            "the folder passed over once for its link, then once a band for its ids"
        );
        assert_eq!(counts.delta_rows, 41 * 4);
        assert_eq!(counts.metas, 1);

        let requests = remote.requests.len();
        sync(&mut store, &mut remote, since("2026-03-01")).unwrap();
        assert!(matches!(
            remote.requests[requests].listing,
            PimdirListing::Delta(_)
        ));
        assert_eq!(remote.graph.counts.delta_rows, 41 * 4, "nothing changed");
        assert_eq!(remote.graph.counts.band_rows, 146);
    }

    /// A message sent above the band its `Date` puts it in stays bound.
    ///
    /// Exchange's submission may lie long after `Date`: a band capped by
    /// `sentDateTime` missed it, and the round found it absent (SYNC §5).
    #[test]
    fn a_band_round_keeps_a_message_sent_after_its_date() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut graph = folder();
        graph.sent_after(
            "late",
            "2026-11-02T10:00:00Z",
            "Wed, 15 Jul 2026 10:00:00 +0000",
        );
        let mut remote = Remote::new(graph);

        sync(&mut store, &mut remote, PimdirScope::unbounded()).unwrap();
        assert!(stored(&store).contains("late"));
        sync(&mut store, &mut remote, since("2026-09-01")).unwrap();
        assert_eq!(coverage(&store), Some(since("2026-09-01")));

        let requests = remote.requests.len();
        sync(&mut store, &mut remote, since("2026-06-01")).unwrap();
        assert!(
            remote.requests[requests..]
                .iter()
                .all(|request| matches!(request.listing, PimdirListing::Round { band: true, .. })),
            "the widening lists its band",
        );
        assert_eq!(coverage(&store), Some(since("2026-06-01")));
        let mut expected = months(&["2026-03", "2026-06", "2026-07", "2026-09"]);
        expected.insert("late".into());
        assert_eq!(stored(&store), expected);
    }

    /// A round under a scope states it narrowed; its pass hands every id.
    ///
    /// pimdir SYNC §4, §5: a band round, widening the coverage, hands them on
    /// its last page too, and a delta states neither.
    #[test]
    fn a_scoped_round_is_narrowed_and_its_pass_enumerates_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut remote = Remote::new(folder());

        sync(&mut store, &mut remote, since("2026-09-01")).unwrap();
        let (pass, band) = remote.stated.split_last().unwrap();
        assert_eq!(*pass, (true, Some(41)), "the pass lists the whole folder");
        assert!(
            band.iter().all(|stated| *stated == (true, None)),
            "{band:?}"
        );

        let pages = remote.stated.len();
        sync(&mut store, &mut remote, since("2026-07-01")).unwrap();
        let (last, widened) = remote.stated[pages..].split_last().unwrap();
        assert_eq!(*last, (true, Some(41)), "the band's last page enumerates");
        assert!(
            widened.iter().all(|stated| *stated == (true, None)),
            "{widened:?}"
        );

        let pages = remote.stated.len();
        sync(&mut store, &mut remote, since("2026-07-01")).unwrap();
        assert_eq!(remote.stated[pages..], [(false, None)], "a delta");
    }

    /// A delta drops what changed outside the scope.
    ///
    /// It follows a bound member's flags, reads the summary of a new member
    /// in scope only, and applies a removal whatever the date.
    #[test]
    fn a_delta_keeps_to_the_scope_and_reads_only_new_members() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut remote = Remote::new(folder());
        sync(&mut store, &mut remote, since("2026-07-01")).unwrap();
        assert_eq!(stored(&store), months(&["2026-07", "2026-09"]));
        let metas = remote.graph.counts.metas;

        remote.graph.flag("2026-09-03");
        remote.graph.flag("2026-03-03");
        remote.graph.add("new", Some("2026-10-06T08:00:00Z"));
        remote.graph.add("old", Some("2020-01-01T08:00:00Z"));
        remote.graph.delete("2026-07-04");
        remote.graph.delete("2026-03-04");

        sync(&mut store, &mut remote, since("2026-07-01")).unwrap();

        let mut expected = months(&["2026-07", "2026-09"]);
        expected.insert("new".into());
        expected.remove("2026-07-04");
        assert_eq!(stored(&store), expected);
        assert!(flags_of(&store, "2026-09-03").contains("\\Flagged"));
        assert_eq!(
            remote.graph.counts.metas - metas,
            1,
            "only the new message in scope has its summary read",
        );
        assert_eq!(remote.graph.counts.passes.len(), 1);
    }

    /// A round broken in its band resumes below it; in its pass, passes again.
    ///
    /// A member added while the band was listed is named by the pass; one a
    /// band page listed and Graph dropped before the pass stays, the pass
    /// stating no removal Graph does not (pimdir SYNC §4).
    #[test]
    fn an_interrupted_round_resumes_its_band_and_passes_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut remote = Remote::new(folder());
        remote.fail_at = Some(2);

        let err = sync(&mut store, &mut remote, since("2026-07-01")).unwrap_err();
        assert!(format!("{err:#}").contains("connection reset"), "{err:#}");
        assert_eq!(
            stored(&store).len(),
            5,
            "two band pages landed, the second listing again the message the first ended at"
        );

        remote.fail_at = Some(remote.requests.len() + 8);
        remote.between = Some((
            remote.requests.len() + 8,
            Box::new(|graph: &mut FakeGraph| {
                graph.delete("2026-09-10");
                graph.add("late", Some("2026-10-06T08:00:00Z"));
            }),
        ));
        let err = sync(&mut store, &mut remote, since("2026-07-01")).unwrap_err();
        assert!(format!("{err:#}").contains("connection reset"), "{err:#}");
        assert_eq!(
            remote.graph.counts.bands, 10,
            "the band resumed, not restarted"
        );
        assert_eq!(
            remote.graph.counts.band_rows, 29,
            "each message listed once, and the one each page ended at again"
        );
        assert!(remote.graph.counts.passes.is_empty());
        assert!(
            stored(&store).contains("2026-09-10"),
            "listed before it went"
        );

        let requests = remote.requests.len();
        sync(&mut store, &mut remote, since("2026-07-01")).unwrap();
        assert_eq!(
            remote.requests[requests].cursor(),
            Some(&PimdirCursor(b"pass".to_vec())),
            "the third run resumes with the pass",
        );
        assert_eq!(remote.graph.counts.passes.len(), 1);
        assert_eq!(remote.graph.counts.bands, 10);

        let mut expected = months(&["2026-07", "2026-09"]);
        expected.insert("late".into());
        assert_eq!(stored(&store), expected, "the pass adds to the band");

        let coverage = store.list_coverage(COLLECTION).unwrap();
        assert!(coverage[0].round.is_none(), "the round closed");
    }

    /// A pass states gone only what its rows mark removed (pimdir SYNC §4).
    ///
    /// A fresh delta ending early drops none of the members it fails to list,
    /// out of the round's scope or listed by a band page of the round.
    #[test]
    fn a_pass_failing_to_list_a_member_states_no_removal() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut remote = Remote::new(folder());
        sync(&mut store, &mut remote, since("2026-06-01")).unwrap();
        let held = stored(&store);
        assert_eq!(held, months(&["2026-06", "2026-07", "2026-09"]));

        let link = String::from_utf8(checkpoint(&store).unwrap()).unwrap();
        let version: usize = link.rsplit('|').next().unwrap().parse().unwrap();
        remote.graph.expired.insert(version);
        remote.graph.unlisted = HashSet::from(["2026-06-03".into(), "2026-07-04".into()]);
        sync(&mut store, &mut remote, since("2026-07-01")).unwrap();

        assert_eq!(remote.graph.counts.passes.len(), 2, "the round passed");
        assert!(store.list_coverage(COLLECTION).unwrap()[0].round.is_none());
        assert_eq!(stored(&store), held, "a member not listed is not removed");
    }

    /// A message Graph dropped between its band page and the pass stays.
    ///
    /// No row states it removed; the next round over its scope finds it
    /// absent.
    #[test]
    fn a_message_dropped_before_the_pass_goes_with_a_later_round() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut remote = Remote::new(folder());
        remote.fail_at = Some(2);
        sync(&mut store, &mut remote, since("2026-09-01")).unwrap_err();
        assert!(
            stored(&store).contains("2026-09-10"),
            "a band page listed it"
        );

        remote.graph.delete("2026-09-10");
        sync(&mut store, &mut remote, since("2026-09-01")).unwrap();
        assert!(store.list_coverage(COLLECTION).unwrap()[0].round.is_none());
        assert!(
            stored(&store).contains("2026-09-10"),
            "stamped, never absent"
        );

        let link = String::from_utf8(checkpoint(&store).unwrap()).unwrap();
        let version: usize = link.rsplit('|').next().unwrap().parse().unwrap();
        remote.graph.expired.insert(version);
        sync(&mut store, &mut remote, since("2026-09-01")).unwrap();
        let mut expected = months(&["2026-09"]);
        expected.remove("2026-09-10");
        assert_eq!(stored(&store), expected, "the next round finds it absent");
    }

    /// A band round broken after a page resumes below the date it ended at.
    ///
    /// A message deleted from the page in between shifts no later one out of
    /// the round, which has no pass to correct it.
    #[test]
    fn a_band_round_resumed_after_a_delete_misses_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut remote = Remote::new(folder());
        sync(&mut store, &mut remote, since("2026-07-01")).unwrap();

        remote.fail_at = Some(remote.requests.len() + 10);
        let err = sync(&mut store, &mut remote, since("2026-06-01")).unwrap_err();
        assert!(format!("{err:#}").contains("connection reset"), "{err:#}");
        assert!(
            stored(&store).contains("2026-06-10"),
            "the first page landed"
        );

        remote.graph.delete("2026-06-10");
        sync(&mut store, &mut remote, since("2026-06-01")).unwrap();
        sync(&mut store, &mut remote, since("2026-06-01")).unwrap();

        let mut expected = months(&["2026-06", "2026-07", "2026-09"]);
        expected.remove("2026-06-10");
        assert_eq!(stored(&store), expected);
    }

    /// Pages sent within one second follow Graph's next link at once.
    ///
    /// The next page would list them again whole, so the same request goes
    /// on until one leaves that second: the round lists each.
    #[test]
    fn a_band_dense_in_one_second_still_goes_on() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut graph = folder();
        let burst: Vec<String> = (1..=7).map(|n| format!("burst-{n}")).collect();
        for id in &burst {
            graph.add(id, Some("2026-06-15T10:00:00Z"));
        }
        let mut remote = Remote::new(graph);
        sync(&mut store, &mut remote, since("2026-07-01")).unwrap();

        sync(&mut store, &mut remote, since("2026-06-01")).unwrap();

        let mut expected = months(&["2026-06", "2026-07", "2026-09"]);
        expected.extend(burst);
        assert_eq!(stored(&store), expected);
    }

    /// With no scope, a round is the delta with summaries.
    ///
    /// A narrower scope later follows the same link; widening back lists the
    /// band below the coverage and keeps it; an expired link opens a round.
    #[test]
    fn a_whole_folder_link_serves_every_scope_after_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut remote = Remote::new(folder());

        sync(&mut store, &mut remote, PimdirScope::unbounded()).unwrap();
        assert_eq!(stored(&store).len(), 41);
        assert_eq!(remote.graph.counts.passes, [SUMMARY_SELECT]);
        assert_eq!(remote.graph.counts.bands, 0);

        remote.graph.flag("2026-03-02");
        remote.graph.flag("2026-09-02");
        remote.graph.add("new", Some("2026-10-06T08:00:00Z"));
        let requests = remote.requests.len();
        sync(&mut store, &mut remote, since("2026-07-01")).unwrap();
        assert!(matches!(
            remote.requests[requests].listing,
            PimdirListing::Delta(_)
        ));
        assert!(flags_of(&store, "2026-09-02").contains("\\Flagged"));
        assert!(
            !flags_of(&store, "2026-03-02").contains("\\Flagged"),
            "a change out of scope is dropped",
        );
        assert!(stored(&store).contains("new"));
        assert_eq!(remote.graph.counts.metas, 0, "summary rows name themselves");
        assert_eq!(stored(&store).len(), 42, "a narrower scope deletes nothing");

        sync(&mut store, &mut remote, PimdirScope::unbounded()).unwrap();
        assert_eq!(
            remote.graph.counts.bands, 20,
            "the band below the coverage, listed with the coverage: 41 dated messages"
        );
        assert_eq!(
            remote.graph.counts.passes.len(),
            2,
            "and the folder's ids enumerated by a fresh delta"
        );
        assert!(
            flags_of(&store, "2026-03-02").contains("\\Flagged"),
            "the band reads what the delta dropped",
        );

        let link = checkpoint(&store).unwrap();
        assert!(
            link.starts_with(b"summary "),
            "the whole-folder link is kept"
        );
        let version: usize = String::from_utf8(link)
            .unwrap()
            .rsplit('|')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        remote.graph.expired.insert(version);
        sync(&mut store, &mut remote, since("2026-09-01")).unwrap();
        assert_eq!(remote.graph.counts.passes.len(), 3, "a new link");
        assert_eq!(
            remote.graph.counts.passes[2], IDS_SELECT,
            "made under the scope"
        );
        assert_eq!(remote.graph.counts.bands, 25);
        assert_eq!(
            stored(&store).len(),
            42,
            "the round deletes nothing out of scope"
        );
    }

    /// A bare link stored under a bounded coverage is refused.
    ///
    /// A `$filter` made it before links were tagged: a round over the scope
    /// makes one over the whole folder.
    #[test]
    fn a_filtered_link_of_before_opens_a_round() {
        let mut graph = folder();
        let mut folder = Folder::default();
        let handles = HashSet::new();
        let coverage = since("2026-09-01");
        let request = PimdirEnumerate {
            listing: PimdirListing::Delta(io_pimdir::collection::PimdirCheckpoint(
                b"delta|summary|0".to_vec(),
            )),
            scope: coverage.clone(),
        };
        let held = Held {
            handles: &handles,
            checkpoint: None,
            coverage: Some(&coverage),
        };
        assert!(matches!(
            enumerate(&mut graph, &mut folder, "INBOX", &request, held).unwrap(),
            Listed::CursorRejected
        ));
        assert_eq!(graph.counts, Counts::default(), "nothing is asked of Graph");
    }

    /// The pass lists again only band members whose marks moved since.
    ///
    /// It reads their summaries; the rest stays listed by the round, and
    /// nothing kept of the band outlives the pass.
    #[test]
    fn the_pass_relists_only_what_moved_since_the_band() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut remote = Remote::new(folder());
        remote.between = Some((
            4,
            Box::new(|graph: &mut FakeGraph| graph.flag("2026-09-05")),
        ));

        sync(&mut store, &mut remote, since("2026-09-01")).unwrap();

        assert_eq!(stored(&store), months(&["2026-09"]));
        assert!(flags_of(&store, "2026-09-05").contains("\\Flagged"));
        assert!(!flags_of(&store, "2026-09-04").contains("\\Flagged"));
        assert_eq!(
            remote.graph.counts.metas, 2,
            "the undated message and the one flagged since the band"
        );
        assert!(remote.folder.banded.is_empty(), "dropped with the pass");
        let coverage = store.list_coverage(COLLECTION).unwrap();
        assert!(coverage[0].round.is_none(), "the round closed");
    }

    /// When the open round of the folder opened, as the store stamped it.
    fn started_at(store: &PimdirSourceStore) -> String {
        store.list_coverage(COLLECTION).unwrap()[0]
            .round
            .as_ref()
            .expect("a round is open")
            .started_at
            .clone()
    }

    fn round_open(store: &PimdirSourceStore) -> bool {
        store.list_coverage(COLLECTION).unwrap()[0].round.is_some()
    }

    fn band_head(since: &str) -> PimdirListing {
        PimdirListing::Head(PimdirHead::Band {
            since: since.to_owned(),
        })
    }

    /// A band head lists from two days before the instant the store stamped.
    ///
    /// However the store wrote it; one that reads as no instant lists none,
    /// and Graph lists no delta head.
    #[test]
    fn a_band_head_lists_from_two_days_before_the_round_opened() {
        for since in [
            "2026-10-13T10:00:00.000Z",
            "2026-10-13T10:00:00Z",
            "2026-10-13T12:00:00+02:00",
            "2026-10-13 10:00:00",
            "2026-10-13 10:00:00.250",
            "2026-10-13T10:00:00.250",
        ] {
            assert_eq!(
                received_floor(since).as_deref(),
                Some("2026-10-11T10:00:00Z"),
                "{since}"
            );
        }
        assert_eq!(received_floor("soon"), None);

        let mut graph = folder();
        let mut folder = Folder::default();
        let handles = HashSet::new();
        let held = || Held {
            handles: &handles,
            checkpoint: None,
            coverage: None,
        };
        for listing in [
            band_head("soon"),
            PimdirListing::Head(PimdirHead::Delta(io_pimdir::collection::PimdirCheckpoint(
                b"summary delta|summary|0".to_vec(),
            ))),
        ] {
            let request = PimdirEnumerate {
                listing,
                scope: PimdirScope::unbounded(),
            };
            assert!(matches!(
                enumerate(&mut graph, &mut folder, "INBOX", &request, held()).unwrap(),
                Listed::NoHead
            ));
        }
        assert_eq!(graph.counts, Counts::default(), "nothing is asked of Graph");
        assert_eq!(folder.head, None);
    }

    /// A whole-folder round resumed lists its head first: arrivals land now.
    ///
    /// Later pages never list an arrival sorting before their position, so the
    /// last page lists the head again: what is still there is kept, a member
    /// the head named that went before is found absent.
    #[test]
    fn a_whole_folder_round_lists_its_head_and_its_last_page_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut remote = Remote::new(folder());
        remote.heads = PimdirHeads::BAND;

        remote.fail_at = Some(2);
        sync(&mut store, &mut remote, PimdirScope::unbounded()).unwrap_err();
        assert_eq!(stored(&store).len(), 6, "two pages of three landed");
        assert!(
            remote.graph.counts.received.is_empty(),
            "a run opening the round lists no head"
        );
        let since = started_at(&store);

        remote.graph.arrive("0-fresh", "2026-10-12T08:00:00Z");
        remote.graph.arrive("zz-brief", "2026-10-12T09:00:00Z");
        let first = remote.requests.len();
        remote.fail_at = Some(first + 2);
        sync(&mut store, &mut remote, PimdirScope::unbounded()).unwrap_err();

        assert_eq!(remote.requests[first].listing, band_head(&since));
        assert!(remote.requests[first + 1].cursor().is_some());
        assert_eq!(
            remote.graph.counts.received,
            [received_floor(&since).unwrap()],
            "the head lists from two days before the round opened",
        );
        let landed = stored(&store);
        assert!(landed.contains("0-fresh"), "the arrival shows at once");
        assert!(landed.contains("zz-brief"));
        assert!(round_open(&store));

        remote.graph.delete("zz-brief");
        remote.graph.arrive("0-late", "2026-10-12T10:00:00Z");
        let first = remote.requests.len();
        sync(&mut store, &mut remote, PimdirScope::unbounded()).unwrap();

        assert_eq!(remote.requests[first].listing, band_head(&since));
        let mut expected = months(&["2026-03", "2026-06", "2026-07", "2026-09"]);
        expected.insert("0-fresh".into());
        expected.insert("0-late".into());
        assert_eq!(
            stored(&store),
            expected,
            "the last page keeps what a head named, the one gone is absent"
        );
        assert_eq!(
            remote.graph.counts.received.len(),
            3,
            "the head of each run resuming the round, and the last page's",
        );
        assert!(!round_open(&store), "the round closed");
        assert!(checkpoint(&store).unwrap().starts_with(b"summary "));
        assert_eq!(remote.folder.head, None, "the last page owes nothing more");

        let requests = remote.requests.len();
        sync(&mut store, &mut remote, PimdirScope::unbounded()).unwrap();
        assert!(matches!(
            remote.requests[requests].listing,
            PimdirListing::Delta(_)
        ));
        assert_eq!(stored(&store), expected, "the next delta keeps them");
    }

    /// Under a scope, the head lands an arrival the band never lists.
    ///
    /// The pass lists it again: a bound member by its flags, one the head
    /// named this run by a summary read. A member the head named that went
    /// before the pass is found absent.
    #[test]
    fn a_scoped_round_lists_its_head_and_its_pass_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut remote = Remote::new(folder());
        remote.heads = PimdirHeads::BAND;
        let scope = since("2026-07-01");

        remote.fail_at = Some(2);
        sync(&mut store, &mut remote, scope.clone()).unwrap_err();
        let since = started_at(&store);

        remote.graph.arrive("fresh", "2026-10-12T08:00:00Z");
        remote.graph.arrive("brief", "2026-07-01T05:00:00Z");
        let first = remote.requests.len();
        remote.fail_at = Some(first + 2);
        sync(&mut store, &mut remote, scope.clone()).unwrap_err();

        assert_eq!(remote.requests[first].listing, band_head(&since));
        let landed = stored(&store);
        assert!(landed.contains("fresh") && landed.contains("brief"));

        remote.graph.delete("brief");
        remote.graph.arrive("late", "2026-10-12T10:00:00Z");
        let metas = remote.graph.counts.metas;
        let first = remote.requests.len();
        sync(&mut store, &mut remote, scope.clone()).unwrap();

        assert_eq!(remote.requests[first].listing, band_head(&since));
        assert_eq!(
            remote.graph.counts.received.len(),
            2,
            "a head a run; the pass lists the whole scope already"
        );
        assert_eq!(
            remote.graph.counts.metas - metas,
            2,
            "the undated message and the arrival the head named this run"
        );
        let mut expected = months(&["2026-07", "2026-09"]);
        expected.insert("fresh".into());
        expected.insert("late".into());
        assert_eq!(stored(&store), expected);
        assert!(!round_open(&store), "the round closed");
        assert_eq!(remote.folder.head, None);
    }

    /// A widening round's last band page lists its head again in the band.
    ///
    /// An arrival above the band's position, which no band page lists, is
    /// kept when the round closes; one gone before it is found absent.
    #[test]
    fn a_band_round_lists_its_head_and_its_last_page_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut remote = Remote::new(folder());
        remote.heads = PimdirHeads::BAND;
        sync(&mut store, &mut remote, since("2026-09-01")).unwrap();
        let link = checkpoint(&store).unwrap();

        let first = remote.requests.len();
        remote.fail_at = Some(first + 2);
        sync(&mut store, &mut remote, since("2026-06-01")).unwrap_err();
        assert!(matches!(
            remote.requests[first].listing,
            PimdirListing::Round { band: true, .. }
        ));
        let since_round = started_at(&store);

        remote.graph.arrive("old", "2026-07-09T12:00:00Z");
        remote.graph.arrive("gone", "2026-06-01T05:00:00Z");
        let first = remote.requests.len();
        remote.fail_at = Some(first + 2);
        sync(&mut store, &mut remote, since("2026-06-01")).unwrap_err();
        assert_eq!(remote.requests[first].listing, band_head(&since_round));
        assert_eq!(
            remote.requests[first].scope,
            since("2026-06-01"),
            "the run's scope"
        );
        let landed = stored(&store);
        assert!(landed.contains("old") && landed.contains("gone"));

        remote.graph.delete("gone");
        let first = remote.requests.len();
        sync(&mut store, &mut remote, since("2026-06-01")).unwrap();

        assert_eq!(remote.requests[first].listing, band_head(&since_round));
        let mut expected = months(&["2026-06", "2026-07", "2026-09"]);
        expected.insert("old".into());
        assert_eq!(stored(&store), expected);
        assert_eq!(remote.graph.counts.received.len(), 3);
        assert!(!round_open(&store), "the round closed");
        assert_eq!(checkpoint(&store), Some(link), "the band keeps the link");
        assert_eq!(coverage(&store), Some(since("2026-06-01")));
    }

    /// What a pass or a delta keeps of a message while it follows the
    /// folder is its id and a few octets, never its row.
    #[test]
    fn a_followed_row_is_kept_as_a_few_octets() {
        assert!(std::mem::size_of::<Seen>() <= 2);
        assert!(std::mem::size_of::<Change>() <= 16);
        assert!(
            std::mem::size_of::<MsgraphMessage>()
                > 8 * std::mem::size_of::<(String, (usize, Seen))>()
        );
    }
}
