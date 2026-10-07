//! # Graph mail listing
//!
//! How a Graph mail folder is listed (pimdir SYNC §4, §5). Graph's message
//! delta takes no ceiling on any date, and a link made under a `$filter`
//! reports the changes of its filter alone, so a scope widened under such a
//! link would list the whole wider scope again. The checkpoint is therefore
//! a delta link made over the whole folder, with no filter, whatever the
//! scope, and the connector is bound to no scope
//! ([`Client::scope_bound`](crate::client::Client::scope_bound)):
//!
//! - **Whole folder** (no scope): a round is the delta itself, with the
//!   summary `$select`, a page per request, its delta link the checkpoint.
//! - **Under a scope**: a round lists the scope's mail by date band, a
//!   plain `/messages` listing filtered on `sentDateTime` (Graph's reading
//!   of the `Date` header, exact), newest first, each page with its summary;
//!   once the band is listed, one pass of the folder's delta with ids,
//!   dates and flags only makes the link, lands as the round's last page,
//!   and corrects what changed or went while the band was listed. A round
//!   over the band a coverage lacks lists that band alone and keeps the
//!   link.
//! - **A delta** follows the stored link: a change outside the scope is
//!   dropped, by `sentDateTime`; a changed member in scope the store does
//!   not bind is named by its summary, read in JSON batches when the link
//!   carries ids only.
//!
//! The link is the same whatever scope a later run asks for: a narrower one
//! keeps it, a wider one lists its band and keeps it. A link made by a
//! whole-folder round carries summaries, one made under a scope ids only;
//! the checkpoint says which. A link stored before they were tagged was made
//! under a `$filter` when the coverage is bounded, and is refused, which
//! opens a round.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, SecondsFormat};
use io_msgraph::v1::{
    client::MsgraphClientStd,
    rest::batch::MSGRAPH_BATCH_MAX_REQUESTS,
    rest::users::messages::{
        MsgraphMessage,
        delta::{MsgraphMessagesDeltaParams, MsgraphMessagesDeltaResponse},
        list::{MsgraphMessagesListParams, MsgraphMessagesListResponse},
    },
    send::{MsgraphSend, user_path},
};
use io_pimdir::{
    collection::PimdirScope,
    remote::{PimdirEnumerate, PimdirListing},
};
use log::{debug, trace, warn};
use url::Url;

use super::{GraphClient, is_expired_link, message_date, message_envelope, message_flags};
use crate::{
    client::{EnumEntry, Enumeration, Held, Listed},
    kind::mail::parse_summary,
};

/// The summary `$select`: what names a message (pimdir STORAGE Annex A.1)
/// and its flags, so a page is small enough to hold a thousand.
pub(super) const SUMMARY_SELECT: &str = "id,subject,from,toRecipients,ccRecipients,sentDateTime,receivedDateTime,isRead,isDraft,flag,hasAttachments,internetMessageId,conversationId,parentFolderId";

/// The `$select` of a link made under a scope: the id, the date the scope
/// reads and what the flags map from, so its first pass over a whole
/// folder stays small.
pub(super) const IDS_SELECT: &str = "id,sentDateTime,isRead,isDraft,flag";

/// The messages a page holds at most: `Prefer: odata.maxpagesize` on every
/// delta request (Graph caps it at 512 for message delta), `$top` on a band
/// listing. With [`SUMMARY_SELECT`] a delta page of a thousand answered in
/// under three seconds (measured on a test tenant, 2026-10-07).
pub(super) const PAGE_SIZE: u32 = 1000;

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

    /// The page a band listing's next link answers, `None` when Graph no
    /// longer serves it.
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

    /// Reads checkpoint bytes back. A bare link, stored before links were
    /// tagged, was made over the whole folder with summaries unless the
    /// coverage is bounded: then a `$filter` made it, and it is refused.
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
    /// The next page of a band listing.
    Band(String),
    /// The band listed, the pass over the folder that makes the link.
    Pass,
}

impl Step {
    fn encode(&self) -> Vec<u8> {
        match self {
            Self::Summary(link) => format!("summary {link}").into_bytes(),
            Self::Band(link) => format!("band {link}").into_bytes(),
            Self::Pass => b"pass".to_vec(),
        }
    }

    /// Reads cursor bytes back. A bare link, stored before cursors were
    /// tagged, is a delta round's next link: whole-folder when `scope` is
    /// unbounded, else made under a `$filter`, and refused.
    fn decode(bytes: &[u8], scope: &PimdirScope) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        if let Some(link) = text.strip_prefix("summary ") {
            return Some(Self::Summary(link.to_owned()));
        }
        if let Some(link) = text.strip_prefix("band ") {
            return Some(Self::Band(link.to_owned()));
        }
        if text == "pass" {
            return Some(Self::Pass);
        }
        if text.is_empty() || !scope.is_unbounded() {
            return None;
        }
        Some(Self::Summary(text.to_owned()))
    }
}

/// Lists one page of the mail folder `mailbox` (pimdir SYNC §4), as the
/// module describes. `rows` caches the summary of every message the run
/// listed in it, which serves the `Meta` tier and names a member a later
/// page lists bare.
pub(super) fn enumerate(
    wire: &mut impl MailWire,
    rows: &mut HashMap<String, MsgraphMessage>,
    mailbox: &str,
    request: &PimdirEnumerate,
    held: Held<'_>,
) -> Result<Listed> {
    let scope = &request.scope;

    match &request.listing {
        PimdirListing::Delta(checkpoint) => match Link::decode(&checkpoint.0, held.coverage) {
            Some(link) => delta(wire, rows, mailbox, &link, scope, held),
            None => {
                warn!("graph delta link of {mailbox} reads as none, opening a round");
                Ok(Listed::CursorRejected)
            }
        },
        PimdirListing::Round { cursor: None, band } => {
            if *band || !scope.is_unbounded() {
                band_page(wire, rows, mailbox, None, *band, scope)
            } else {
                summary_page(wire, rows, mailbox, None)
            }
        }
        PimdirListing::Round {
            cursor: Some(cursor),
            band,
        } => match Step::decode(&cursor.0, scope) {
            Some(Step::Summary(next)) if scope.is_unbounded() => {
                summary_page(wire, rows, mailbox, Some(&next))
            }
            Some(Step::Band(next)) => band_page(wire, rows, mailbox, Some(&next), *band, scope),
            Some(Step::Pass) if !band => pass(wire, rows, mailbox, scope, held),
            _ => Ok(Listed::CursorRejected),
        },
    }
}

/// One page of a whole-folder round: the delta with summaries, from its
/// start or its next link; the last page carries the delta link.
fn summary_page(
    wire: &mut impl MailWire,
    rows: &mut HashMap<String, MsgraphMessage>,
    mailbox: &str,
    next: Option<&str>,
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
            (message, true) => listing.vanish(rows, message.id),
            (message, false) => listing.named(rows, message),
        }
    }
    trace!(
        "graph delta page of {mailbox}: {} rows",
        listing.items.len()
    );

    Ok(Listed::Page(listing.page(true, cursor, checkpoint)))
}

/// One page of a band listing: the messages of `scope` by `sentDateTime`,
/// newest first, each with its summary. Past its last page, a band round
/// ends; a round over the whole scope goes on with the pass. A message with
/// no date, which a filter on `sentDateTime` never lists, stays bound: a
/// band round infers no delete of an undated member (pimdir SYNC §5).
fn band_page(
    wire: &mut impl MailWire,
    rows: &mut HashMap<String, MsgraphMessage>,
    mailbox: &str,
    next: Option<&str>,
    band: bool,
    scope: &PimdirScope,
) -> Result<Listed> {
    let page = match next {
        None => wire.band_first(mailbox, scope)?,
        Some(next) => match wire.band_next(next)? {
            Some(page) => page,
            None => return Ok(Listed::CursorRejected),
        },
    };

    let cursor = match (page.next_link, band) {
        (Some(next), _) => Some(Step::Band(next)),
        (None, true) => None,
        (None, false) => Some(Step::Pass),
    };

    let mut listing = Listing::default();
    for message in page.value {
        if in_scope(scope, &message) {
            listing.named(rows, message);
        }
    }
    trace!(
        "graph band page of {mailbox}: {} messages",
        listing.items.len()
    );

    Ok(Listed::Page(listing.page(
        true,
        cursor.map(|step| step.encode()),
        None,
    )))
}

/// The round's last page under a scope: one pass of a fresh delta over the
/// whole folder with ids, dates and flags, whose delta link becomes the
/// checkpoint. Every member in scope is listed, by its binding, by the
/// summary the band read, else by one read in batches; a member the store
/// binds or the band listed that the pass does not list is gone.
///
/// The pass runs within one request of the engine: resumed, it starts
/// again, since what it did not list is what went.
fn pass(
    wire: &mut impl MailWire,
    rows: &mut HashMap<String, MsgraphMessage>,
    mailbox: &str,
    scope: &PimdirScope,
    held: Held<'_>,
) -> Result<Listed> {
    debug!("pass over the graph folder {mailbox} for its delta link");

    let (listed, delta) =
        follow(wire, mailbox, None)?.with_context(|| format!("Delta pass of {mailbox} expired"))?;

    let mut listing = Listing::default();
    let mut present = HashSet::new();
    let mut unread = Vec::new();
    for (message, removed) in latest(listed) {
        if removed {
            listing.vanish(rows, message.id);
            continue;
        }
        present.insert(message.id.clone());
        if !in_scope(scope, &message) {
            continue;
        }
        if held.holds(&message.id) {
            listing.bare(&message);
        } else if let Some(known) = rows.get_mut(&message.id) {
            known.is_read = message.is_read;
            known.is_draft = message.is_draft;
            known.flag = message.flag.clone();
            let known = known.clone();
            listing.named(rows, known);
        } else {
            unread.push(message.id);
        }
    }

    listing.read_metas(wire, rows, &unread, scope)?;

    let gone: Vec<String> = held
        .handles
        .iter()
        .chain(rows.keys())
        .filter(|id| !present.contains(*id))
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    for id in gone {
        listing.vanish(rows, id);
    }

    Ok(Listed::Page(listing.page(
        true,
        None,
        Some(Link::Ids(delta).encode()),
    )))
}

/// What changed since `link`: removals vanish whatever the scope, changes
/// outside it are dropped, a change of a member the store binds is listed
/// by its flags, and a member it does not bind is named by its summary,
/// the row's own or one read in batches.
fn delta(
    wire: &mut impl MailWire,
    rows: &mut HashMap<String, MsgraphMessage>,
    mailbox: &str,
    link: &Link,
    scope: &PimdirScope,
    held: Held<'_>,
) -> Result<Listed> {
    let Some((listed, next)) = follow(wire, mailbox, Some(link.as_str()))? else {
        warn!("graph delta link of {mailbox} expired, opening a round");
        return Ok(Listed::CursorRejected);
    };

    let mut listing = Listing::default();
    let mut unread = Vec::new();
    for (message, removed) in latest(listed) {
        if removed {
            listing.vanish(rows, message.id);
            continue;
        }
        if !in_scope(scope, &message) {
            trace!("change of {} out of scope, dropped", message.id);
            continue;
        }
        match link {
            Link::Summary(_) => listing.named(rows, message),
            Link::Ids(_) if held.holds(&message.id) => listing.bare(&message),
            Link::Ids(_) => unread.push(message.id),
        }
    }

    listing.read_metas(wire, rows, &unread, scope)?;

    Ok(Listed::Page(listing.page(
        false,
        None,
        Some(link.moved(next).encode()),
    )))
}

/// Follows a delta from `link`, or from a fresh pass with ids only, to its
/// delta link: every row, flagged when removed, and the delta link. `None`
/// when Graph answers that the link expired.
fn follow(
    wire: &mut impl MailWire,
    mailbox: &str,
    link: Option<&str>,
) -> Result<Option<(Vec<Row>, String)>> {
    let mut page = match link {
        None => wire.delta_first(mailbox, IDS_SELECT)?,
        Some(link) => match wire.delta_next(link)? {
            Some(page) => page,
            None => return Ok(None),
        },
    };

    let mut rows = Vec::new();
    loop {
        rows.extend(
            page.value
                .into_iter()
                .map(|row| (row.message, row.removed.is_some())),
        );
        if let Some(delta) = page.delta_link {
            return Ok(Some((rows, delta)));
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

/// The `$filter` of a band listing: `sentDateTime` from the scope's floor,
/// inclusive, below its ceiling, exclusive, as the scope reads the `Date`.
/// A bound that does not read as an instant is left out, a superset the
/// local check narrows.
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

/// A page being built: its entries, its removals and its meta octets.
#[derive(Default)]
struct Listing {
    items: Vec<EnumEntry>,
    vanished: Vec<String>,
    bytes: u64,
}

impl Listing {
    /// Lists `message` named by its summary, which the cache keeps.
    fn named(&mut self, rows: &mut HashMap<String, MsgraphMessage>, message: MsgraphMessage) {
        self.bytes += serde_json::to_vec(&message).map_or(0, |json| json.len() as u64);
        let summary = message_envelope(&message.id, &message);
        self.items.push(EnumEntry {
            id: message.id.clone(),
            flags: message_flags(&message),
            revision: None,
            meta: Some(parse_summary(&summary)),
        });
        rows.insert(message.id.clone(), message);
    }

    /// Lists a member the store binds by its flags, the store naming it.
    fn bare(&mut self, message: &MsgraphMessage) {
        self.bytes += serde_json::to_vec(message).map_or(0, |json| json.len() as u64);
        self.items.push(EnumEntry {
            id: message.id.clone(),
            flags: message_flags(message),
            revision: None,
            meta: None,
        });
    }

    /// States `id` removed.
    fn vanish(&mut self, rows: &mut HashMap<String, MsgraphMessage>, id: String) {
        rows.remove(&id);
        self.vanished.push(id);
    }

    /// Reads the summaries of `ids` and lists those in scope; a message
    /// gone since is left out, any other one Graph did not answer fails
    /// the listing, which the next run lists again.
    fn read_metas(
        &mut self,
        wire: &mut impl MailWire,
        rows: &mut HashMap<String, MsgraphMessage>,
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
                self.named(rows, message);
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
            ..Default::default()
        };
        self.op(|client| client.messages_list(Some(&folder), &params))
            .with_context(|| format!("List {mailbox} by date error"))
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

    fn delta_first(&mut self, mailbox: &str, select: &str) -> Result<MsgraphMessagesDeltaResponse> {
        let folder = self.folder_id(mailbox)?;
        let params = MsgraphMessagesDeltaParams {
            select: Some(select),
            filter: None,
            max_page_size: Some(PAGE_SIZE),
        };
        self.op(|client| client.messages_delta_with_params(Some(&folder), &params))
            .with_context(|| format!("Start delta of {mailbox} error"))
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
        let user = user_path(&self.inner.user_id);
        let mut metas = Vec::with_capacity(ids.len());

        for chunk in ids.chunks(MSGRAPH_BATCH_MAX_REQUESTS) {
            let answer = self
                .batch_get(
                    chunk,
                    |id| format!("/{user}/messages/{id}?$select={SUMMARY_SELECT}"),
                    |id, body| {
                        let message = body
                            .and_then(|body| serde_json::from_value::<MsgraphMessage>(body).ok());
                        if message.is_none() {
                            warn!("graph batch summary of {id} does not read as a message");
                        }
                        message
                    },
                )
                .context("Send summary batch error")?;

            let missing = chunk
                .iter()
                .filter(|id| !answer.bodies.contains_key(*id) && !answer.gone.contains(id))
                .count();
            if missing > 0 {
                bail!("Graph did not answer the summary of {missing} changed messages");
            }

            for id in chunk {
                if let Some(mut message) = answer.bodies.get(id).cloned() {
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
        load::PimdirLoadScope,
        placement::{PimdirHandle, PimdirLinkId, PimdirSortKey},
        remote::{
            PimdirEnumerated, PimdirFetchedItem, PimdirPushResult, PimdirRemote, PimdirRemoteItem,
            PimdirRemoteMeta, PimdirTier,
        },
        sync::{PimdirSync, PimdirSyncOptions},
    };

    use super::*;
    use crate::offline::{
        remote::{keep_in_scope, snapshot},
        run_verb,
        storage::load_side,
    };

    const COLLECTION: &str = "msgraph/INBOX";

    // --- the band filter and the links -----------------------------------

    /// A band reads `sentDateTime` exactly as the scope reads the `Date`:
    /// from its floor, inclusive, below its ceiling, exclusive, in UTC.
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
            "no margin: the date Graph filters is the one the scope reads",
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
            Step::Band(link.into()),
            Step::Pass,
            Step::Summary(link.into()),
        ] {
            assert_eq!(Step::decode(&step.encode(), &bounded), Some(step.clone()));
        }
    }

    // --- a fake Graph folder ---------------------------------------------

    /// One message of the fake folder.
    #[derive(Clone)]
    struct Message {
        sent: Option<String>,
        read: bool,
        flagged: bool,
    }

    /// What the fake answered, by kind of request.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Counts {
        /// Band listings started.
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
    }

    /// A Graph mail folder in memory: band listings by `sentDateTime`,
    /// newest first, and a delta over the whole folder whose links are the
    /// version of its change log, `page` rows a page.
    struct FakeGraph {
        page: usize,
        messages: BTreeMap<String, Message>,
        /// Every change, by id, in order: a link at version `v` reports
        /// the ids changed from `log[v]` on.
        log: Vec<String>,
        /// The versions whose links Graph no longer serves.
        expired: HashSet<usize>,
        counts: Counts,
    }

    impl FakeGraph {
        fn new(page: usize) -> Self {
            Self {
                page,
                messages: BTreeMap::new(),
                log: Vec::new(),
                expired: HashSet::new(),
                counts: Counts::default(),
            }
        }

        fn add(&mut self, id: &str, sent: Option<&str>) {
            self.messages.insert(
                id.to_owned(),
                Message {
                    sent: sent.map(String::from),
                    read: false,
                    flagged: false,
                },
            );
            self.log.push(id.to_owned());
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
                "isRead": message.read,
                "isDraft": false,
                "flag": { "flagStatus": if message.flagged { "flagged" } else { "notFlagged" } },
            });
            if summary {
                row["subject"] = serde_json::json!(format!("Subject {id}"));
                row["internetMessageId"] = serde_json::json!(format!("<{id}@example.org>"));
                row["hasAttachments"] = serde_json::json!(false);
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
            MsgraphMessagesListResponse { value, next_link }
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
            }
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

        fn band_next(&mut self, link: &str) -> Result<Option<MsgraphMessagesListResponse>> {
            let parts: Vec<&str> = link.split('|').collect();
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
            let ids: Vec<String> = self.messages.keys().cloned().collect();
            Ok(self.delta_page(&ids, 0, summary, |end| format!("pass|{summary}|{end}")))
        }

        fn delta_next(&mut self, link: &str) -> Result<Option<MsgraphMessagesDeltaResponse>> {
            self.counts.delta_pages += 1;
            let parts: Vec<&str> = link.split('|').collect();
            match parts[0] {
                "pass" => {
                    let summary = parts[1] == "true";
                    let ids: Vec<String> = self.messages.keys().cloned().collect();
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

    // --- the engine over the fake ----------------------------------------

    /// A change made to the fake folder between two enumerations.
    type Change = Box<dyn FnOnce(&mut FakeGraph)>;

    /// The fake Graph folder behind the engine's seam, naming a member
    /// listed bare by its binding as `PimRemote` does, and failing the
    /// enumeration numbered `fail_at`, as a dropped connection would.
    struct Remote {
        graph: FakeGraph,
        rows: HashMap<String, MsgraphMessage>,
        handles: HashSet<String>,
        names: HashMap<String, (PimdirLinkId, PimdirSortKey)>,
        checkpoint: Option<Vec<u8>>,
        coverage: Option<PimdirScope>,
        requests: Vec<PimdirEnumerate>,
        fail_at: Option<usize>,
        /// Run between two enumerations, once, before the one numbered by
        /// its key.
        between: Option<(usize, Change)>,
    }

    impl Remote {
        fn new(graph: FakeGraph) -> Self {
            Self {
                graph,
                rows: HashMap::new(),
                handles: HashSet::new(),
                names: HashMap::new(),
                checkpoint: None,
                coverage: None,
                requests: Vec::new(),
                fail_at: None,
                between: None,
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
            let page = match enumerate(&mut self.graph, &mut self.rows, "INBOX", &request, held)? {
                Listed::Page(page) => page,
                Listed::CursorRejected => return Ok(PimdirEnumerated::CursorRejected),
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

            Ok(PimdirEnumerated::Page(snapshot(
                page.complete,
                page.cursor,
                page.checkpoint,
                items,
                vanished,
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
    }

    fn store(dir: &std::path::Path) -> PimdirSourceStore {
        let store = PimdirStore::open(dir).unwrap().for_source("msgraph");
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
                &PimdirLoadScope::All,
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
            .scope_bound(false);
        run_verb(store, remote, verb).map(|_| ())
    }

    fn loaded(store: &PimdirSourceStore) -> io_pimdir::load::PimdirLoaded {
        store
            .load(
                &PimdirCollectionId(COLLECTION.into()),
                &PimdirLoadScope::Handles(Vec::new()),
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

    /// Three widenings list each message once, by its band, and keep the
    /// link the first round made: the folder is passed over once, and a
    /// run within the coverage follows the link.
    #[test]
    fn widenings_list_their_band_and_keep_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut remote = Remote::new(folder());

        sync(&mut store, &mut remote, since("2026-09-01")).unwrap();
        assert_eq!(stored(&store), months(&["2026-09"]));
        let counts = remote.graph.counts.clone();
        assert_eq!(counts.bands, 1);
        assert_eq!(counts.band_rows, 10, "the band lists its ten messages");
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
        assert_eq!(counts.bands, 4);
        assert_eq!(counts.band_rows, 40, "each dated message listed once");
        assert_eq!(counts.passes.len(), 1, "the folder passed over once");
        assert_eq!(counts.delta_rows, 41);
        assert_eq!(counts.metas, 1);

        let requests = remote.requests.len();
        sync(&mut store, &mut remote, since("2026-03-01")).unwrap();
        assert!(matches!(
            remote.requests[requests].listing,
            PimdirListing::Delta(_)
        ));
        assert_eq!(remote.graph.counts.delta_rows, 41, "nothing changed");
        assert_eq!(remote.graph.counts.band_rows, 40);
    }

    /// A delta drops what changed outside the scope, follows a bound
    /// member's flags, reads the summary of a new member in scope only,
    /// and applies a removal whatever the date.
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

    /// A round broken in its band resumes from the band's next link; broken
    /// in its pass, it passes again, the band left alone; a member deleted
    /// while the band was listed vanishes with the pass, and one added then
    /// is named by it.
    #[test]
    fn an_interrupted_round_resumes_its_band_and_passes_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store(dir.path());
        let mut remote = Remote::new(folder());
        remote.fail_at = Some(2);

        let err = sync(&mut store, &mut remote, since("2026-07-01")).unwrap_err();
        assert!(format!("{err:#}").contains("connection reset"), "{err:#}");
        assert_eq!(stored(&store).len(), 6, "two band pages landed");

        // NOTE: the band holds 20 messages, 7 pages of 3: the pass is the
        // eighth request of the round, the third of this run.
        remote.fail_at = Some(remote.requests.len() + 5);
        remote.between = Some((
            remote.requests.len() + 5,
            Box::new(|graph: &mut FakeGraph| {
                graph.delete("2026-09-10");
                graph.add("late", Some("2026-10-06T08:00:00Z"));
            }),
        ));
        let err = sync(&mut store, &mut remote, since("2026-07-01")).unwrap_err();
        assert!(format!("{err:#}").contains("connection reset"), "{err:#}");
        assert_eq!(
            remote.graph.counts.bands, 1,
            "the band resumed, not restarted"
        );
        assert_eq!(
            remote.graph.counts.band_rows, 20,
            "each message listed once"
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
        assert_eq!(remote.graph.counts.bands, 1);

        let mut expected = months(&["2026-07", "2026-09"]);
        expected.remove("2026-09-10");
        expected.insert("late".into());
        assert_eq!(stored(&store), expected, "the pass corrects the band");

        let coverage = store.list_coverage(COLLECTION).unwrap();
        assert!(coverage[0].round.is_none(), "the round closed");
    }

    /// With no scope, a round is the delta with summaries, as before. A
    /// narrower scope later follows the same link; widening back lists the
    /// band below the coverage and keeps it; an expired link opens a round
    /// over the scope asked for.
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
        assert_eq!(remote.graph.counts.bands, 1, "the band below the coverage");
        assert_eq!(remote.graph.counts.passes.len(), 1, "and the link kept");
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
        assert_eq!(remote.graph.counts.passes.len(), 2, "a new link");
        assert_eq!(
            remote.graph.counts.passes[1], IDS_SELECT,
            "made under the scope"
        );
        assert_eq!(remote.graph.counts.bands, 2);
        assert_eq!(
            stored(&store).len(),
            42,
            "the round deletes nothing out of scope"
        );
    }

    /// A bare link stored under a bounded coverage, made under a `$filter`
    /// before links were tagged, is refused: a round over the scope makes
    /// one over the whole folder.
    #[test]
    fn a_filtered_link_of_before_opens_a_round() {
        let mut graph = folder();
        let mut rows = HashMap::new();
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
            enumerate(&mut graph, &mut rows, "INBOX", &request, held).unwrap(),
            Listed::CursorRejected
        ));
        assert_eq!(graph.counts, Counts::default(), "nothing is asked of Graph");
    }
}
