//! # IMAP adapter
//!
//! Thin glue over [`ImapClient`], which already wraps io_imap's high-level
//! session. The only real work is converting between the shared
//! [`crate::item`] types and io_imap's wire types.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    num::NonZeroU32,
    ops::RangeInclusive,
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::DateTime;
use io_imap::{
    client::{ImapClient as _, ImapClientError},
    rfc3501::{
        append::ImapMessageAppendOptions, copy::ImapMessageCopyOptions,
        fetch::ImapMessageFetchOptions, search::ImapMessageSearchOptions,
        select::ImapMailboxSelectOptions, store::ImapMessageStoreOptions,
    },
    rfc6851::r#move::ImapMessageMoveOptions,
    send::ImapOutput,
    types::{
        core::{AString, Atom, QuotedChar, Vec1},
        datetime::NaiveDate as ImapDate,
        fetch::{MacroOrMessageDataItemNames, MessageDataItem, MessageDataItemName, Section},
        flag::{Flag as ImapFlag, FlagFetch, FlagNameAttribute, StoreType},
        mailbox::{ListMailbox, Mailbox as ImapMailbox},
        search::SearchKey,
        sequence::SequenceSet,
        status::{StatusDataItem, StatusDataItemName},
    },
};
use io_pimdir::{
    collection::PimdirScope,
    remote::{PimdirEnumerate, PimdirHead, PimdirListing},
    summary::{PimdirDerivation, mail},
};
use log::{debug, warn};

use crate::{
    client::{EnumEntry, Enumeration, Gone, Held, Listed, Renamed},
    imap::client::{ImapClient, RoundUids},
    item::{
        collection::Collection,
        flag::{Flag, FlagOp, IanaFlag},
    },
    offline::invitation::Refusal,
};

impl ImapClient {
    /// Lists every selectable mailbox, following each row with a STATUS to
    /// populate totals and unread counts when `with_counts`.
    pub fn list_mailboxes(&mut self, with_counts: bool) -> Result<Vec<Collection>> {
        let reference: ImapMailbox<'static> = ""
            .try_into()
            .map_err(|_| anyhow!("Invalid IMAP list reference"))?;
        let pattern: ListMailbox<'static> = "*"
            .try_into()
            .map_err(|_| anyhow!("Invalid IMAP list pattern"))?;

        let rows = data_of("LIST", self.list(reference, pattern)?);

        let mut mailboxes: Vec<Collection> = rows
            .into_iter()
            .filter(is_selectable)
            .map(mailbox_from)
            .collect();

        if with_counts {
            for mailbox in &mut mailboxes {
                let mbox = parse_mailbox(&mailbox.id)?;
                let items = self.status(
                    mbox,
                    [StatusDataItemName::Messages, StatusDataItemName::Unseen][..].into(),
                )?;
                apply_status(mailbox, data_of("STATUS", items));
            }
        }

        Ok(mailboxes)
    }

    /// The role each mailbox states, by mailbox name: the SPECIAL-USE
    /// attributes of the `LIST` rows (RFC 6154), and `INBOX` (RFC 3501
    /// §5.1). A check-only probe: the shared [`Collection`] stays free of
    /// protocol data.
    pub fn mailbox_roles(&mut self) -> Result<BTreeMap<String, String>> {
        let reference: ImapMailbox<'static> = ""
            .try_into()
            .map_err(|_| anyhow!("Invalid IMAP list reference"))?;
        let pattern: ListMailbox<'static> = "*"
            .try_into()
            .map_err(|_| anyhow!("Invalid IMAP list pattern"))?;

        Ok(data_of("LIST", self.list(reference, pattern)?)
            .into_iter()
            .filter(is_selectable)
            .filter_map(|row| {
                let role = role_of(&row)?;
                Some((mailbox_from(row).id, role.to_owned()))
            })
            .collect())
    }

    /// Lists one page of a mailbox (pimdir SYNC §4).
    ///
    /// A delta reads what changed since the checkpoint's modseq through a
    /// QRESYNC `SELECT`; a server without QRESYNC answers the first page of
    /// a round instead. Any listing meeting a `UIDVALIDITY` other than the
    /// one the store bound its handles under answers [`Listed::Rekey`]. A
    /// round lists the scope newest first, [`PAGE_SIZE`] UIDs a page, its
    /// cursor the lowest UID listed and the highest UID the round found
    /// when it opened; its first page carries the checkpoint taken at the
    /// `SELECT`, so what moves during the round is its heads' or the next
    /// delta's, and its last page lists again what arrived since it opened.
    ///
    /// A head lists what changed while the round is open: a delta head the
    /// QRESYNC delta from a checkpoint, a band head what `SEARCH SINCE`
    /// dates from the round's opening less two days.
    pub fn enumerate(
        &mut self,
        mailbox: &str,
        request: &PimdirEnumerate,
        held: Held<'_>,
    ) -> Result<Listed> {
        match &request.listing {
            PimdirListing::Head(PimdirHead::Delta(checkpoint)) => {
                self.delta_head(mailbox, &checkpoint.0, &held)
            }
            PimdirListing::Head(PimdirHead::Band { since }) => {
                self.band_head(mailbox, since, &held)
            }
            PimdirListing::Delta(checkpoint) => {
                self.delta_page(mailbox, &checkpoint.0, &request.scope, &held)
            }
            PimdirListing::Round { cursor, band } => self.round_page(
                mailbox,
                cursor.as_ref().map(|cursor| cursor.0.as_slice()),
                *band,
                &request.scope,
                &held,
            ),
        }
    }

    /// What changed since `checkpoint`, or a round's first page when the
    /// server cannot say; a rekey when the `UIDVALIDITY` moved.
    fn delta_page(
        &mut self,
        mailbox: &str,
        checkpoint: &[u8],
        scope: &PimdirScope,
        held: &Held<'_>,
    ) -> Result<Listed> {
        let Some(position) = qresync_position(checkpoint, self.supports_qresync()) else {
            return self.round_page(mailbox, None, false, scope, held);
        };
        match self.qresync_delta(mailbox, position, held)? {
            Some(page) => Ok(Listed::Page(page)),
            None => {
                debug!("UIDVALIDITY of {mailbox} moved, naming a rekey");
                Ok(Listed::Rekey)
            }
        }
    }

    /// The delta head of an open round: what changed since `checkpoint`,
    /// answered as a delta and never as a round (pimdir SYNC §4).
    ///
    /// A server without QRESYNC, or a checkpoint with no modseq to list
    /// from, lists none. A checkpoint whose `UIDVALIDITY` moved names a
    /// rekey, as a delta meeting it does (pimdir SYNC §4).
    fn delta_head(&mut self, mailbox: &str, checkpoint: &[u8], held: &Held<'_>) -> Result<Listed> {
        let Some(position) = qresync_position(checkpoint, self.supports_qresync()) else {
            return Ok(Listed::NoHead);
        };
        match self.qresync_delta(mailbox, position, held)? {
            Some(page) => Ok(Listed::Page(page)),
            None => {
                debug!("UIDVALIDITY of {mailbox} moved under the round, naming a rekey");
                Ok(Listed::Rekey)
            }
        }
    }

    /// The QRESYNC delta from `(UIDVALIDITY, modseq)`: the messages changed
    /// since, the meta read for the ones the store does not bind, the UIDs
    /// vanished, and the checkpoint the server holds now. `None` when the
    /// `UIDVALIDITY` moved, its UIDs naming other messages.
    fn qresync_delta(
        &mut self,
        mailbox: &str,
        (validity, modseq): (NonZeroU32, u64),
        held: &Held<'_>,
    ) -> Result<Option<Enumeration>> {
        let mbox = parse_mailbox(mailbox)?;
        let data = data_of("SELECT", self.select_delta(mbox, validity, modseq)?);
        self.mark_selected(mailbox);
        let uid_validity = data.uid_validity.map(|v| v.get()).unwrap_or(validity.get());
        if uid_validity != validity.get() {
            return Ok(None);
        }
        let highest_mod_seq = data.highest_mod_seq.unwrap_or(modseq);

        let changed: Vec<Spine> = data
            .changed
            .iter()
            .filter_map(|fetch| spine(&fetch.items.clone().into_inner()))
            .collect();
        let vanished = held_vanished(&data.vanished_earlier, held);

        // NOTE: a head's checkpoint may be newer than the store's, whose
        // handles then name what an older `UIDVALIDITY` numbered.
        let trusted = trusts(held, uid_validity);
        let unheld: Vec<u32> = changed
            .iter()
            .filter(|row| !(trusted && held.holds(&row.uid.to_string())))
            .map(|row| row.uid)
            .collect();
        let (metas, bytes) = self.fetch_metas(&unheld)?;
        let items = entries(changed, metas);

        let mut page = Enumeration::delta(
            items,
            vanished,
            encode_checkpoint(uid_validity, highest_mod_seq),
        );
        page.bytes = bytes;
        Ok(Some(page))
    }

    /// The band head of an open round: every message the server dates
    /// from `since` less two days by its arrival (`UID SEARCH SINCE`, its
    /// `INTERNALDATE`), read now as a round's page reads it (pimdir SYNC
    /// §4). It states no removal and carries no checkpoint; a message moved
    /// in keeps its arrival and waits for the round's last page. An instant
    /// it cannot read lists none.
    fn band_head(&mut self, mailbox: &str, since: &str, held: &Held<'_>) -> Result<Listed> {
        let Some(day) = imap_day(since, -2) else {
            debug!("no band head of {mailbox} from {since}, not an RFC 3339 instant");
            return Ok(Listed::NoHead);
        };

        let mbox = parse_mailbox(mailbox)?;
        let select = data_of(
            "SELECT",
            self.select(mbox, ImapMailboxSelectOptions::default())?,
        );
        self.mark_selected(mailbox);
        let uid_validity = select.uid_validity.map(|v| v.get()).unwrap_or(0);
        if renumbered(held, uid_validity) {
            debug!("UIDVALIDITY of {mailbox} moved under the round, naming a rekey");
            return Ok(Listed::Rekey);
        }

        let uids: Vec<u32> = if select.exists == Some(0) {
            Vec::new()
        } else {
            let found = self.search(
                Vec1::from(SearchKey::Since(day)),
                ImapMessageSearchOptions { uid: true },
            )?;
            rows_of("SEARCH", found)?
                .into_iter()
                .map(NonZeroU32::get)
                .collect()
        };

        let (items, bytes) = self.list_uids(&uids, uid_validity, held)?;
        Ok(Listed::Page(Enumeration {
            items,
            bytes,
            ..Default::default()
        }))
    }

    /// One page of a round over `scope`, from its start or from `cursor`.
    fn round_page(
        &mut self,
        mailbox: &str,
        cursor: Option<&[u8]>,
        band: bool,
        scope: &PimdirScope,
        held: &Held<'_>,
    ) -> Result<Listed> {
        let cursor = match cursor {
            None => None,
            Some(bytes) => match decode_cursor(bytes) {
                Some(cursor) => Some(cursor),
                None => return Ok(Listed::CursorRejected),
            },
        };

        let mbox = parse_mailbox(mailbox)?;
        let select = data_of(
            "SELECT",
            self.select(mbox, ImapMailboxSelectOptions::default())?,
        );
        self.mark_selected(mailbox);
        let uid_validity = select.uid_validity.map(|v| v.get()).unwrap_or(0);
        let highest_mod_seq = select.highest_mod_seq.unwrap_or(0);
        // NOTE: a server may leave `EXISTS` out, which says nothing of the size.
        let empty = select.exists == Some(0);

        if renumbered(held, uid_validity) {
            debug!("UIDVALIDITY of {mailbox} moved, naming a rekey");
            return Ok(Listed::Rekey);
        }
        // NOTE: the store already rekeyed under the new `UIDVALIDITY`, so
        // only the round's cursor names the old one.
        if let Some(cursor) = cursor
            && cursor.uid_validity != uid_validity
        {
            debug!("UIDVALIDITY of {mailbox} moved under the round, restarting it");
            return Ok(Listed::CursorRejected);
        }

        let key = (scope.since.clone(), scope.until.clone());
        let below = cursor.map(|cursor| cursor.lowest);
        let cached = self.round.take().filter(|round| {
            round.mailbox == mailbox
                && round.uid_validity == uid_validity
                && round.scope == key
                && below.is_some()
        });
        let mut left = match cached {
            Some(round) => round
                .left
                .into_iter()
                .filter(|uid| below.is_none_or(|below| *uid < below))
                .collect(),
            None => self.search_round(scope, None, below, empty)?,
        };
        // NOTE: the highest UID the round found when it opened bounds what
        // arrived since, UIDs only ever growing under one `UIDVALIDITY`.
        let top = match cursor {
            Some(cursor) => cursor.top,
            None => left.first().copied().unwrap_or(0),
        };

        let rest = left.split_off(left.len().min(PAGE_SIZE));
        let mut page_uids = left;
        let next = rest.first().and(page_uids.last()).map(|lowest| {
            encode_cursor(RoundCursor {
                uid_validity,
                lowest: *lowest,
                top,
            })
        });
        if !rest.is_empty() {
            self.round = Some(RoundUids {
                mailbox: mailbox.to_owned(),
                uid_validity,
                scope: key,
                left: rest,
            });
        }

        // NOTE: a band head stamps nothing, so the last page of a round
        // lists again every message that arrived since the round opened,
        // read after this page's `SELECT` (pimdir SYNC §4); a round of one
        // page found them already.
        if next.is_none() && cursor.is_some() {
            page_uids.extend(self.search_round(scope, Some(top), None, empty)?);
            page_uids.sort_unstable_by(|a, b| b.cmp(a));
            page_uids.dedup();
        }

        let (items, bytes) = self.list_uids(&page_uids, uid_validity, held)?;

        // NOTE: a date filter misses what the server dates otherwise than
        // the store, so a narrowed round's last page hands every UID of the
        // mailbox, which no date filter narrowed (pimdir SYNC §4, §5), read
        // after the page's listing: a message arrived since is in it and
        // absent from nothing. A band round hands none: the engine finds
        // absent there only a member dated in the band, below the round's
        // opening, which the enumeration could not keep.
        let narrowed = narrows(scope);
        let enumerated = match narrowed && !band && next.is_none() {
            true => Some(self.search_all(empty)?),
            false => None,
        };

        let first = cursor.is_none();
        Ok(Listed::Page(Enumeration {
            items,
            vanished: Vec::new(),
            complete: true,
            cursor: next,
            checkpoint: (first && !band).then(|| encode_checkpoint(uid_validity, highest_mod_seq)),
            bytes,
            narrowed,
            enumerated,
        }))
    }

    /// Every UID of the selected mailbox, by `UID SEARCH ALL`, as handles.
    /// None when the `SELECT` stated it `empty`.
    fn search_all(&mut self, empty: bool) -> Result<Vec<String>> {
        if empty {
            return Ok(Vec::new());
        }

        let found = self.search(
            Vec1::from(SearchKey::All),
            ImapMessageSearchOptions { uid: true },
        )?;
        let uids = rows_of("SEARCH", found)?;
        Ok(uids.into_iter().map(|uid| uid.get().to_string()).collect())
    }

    /// What a UID set names, read now: the flags of the messages the store
    /// binds under this `UIDVALIDITY`, the meta of the others, and the
    /// header octets read. A UID the mailbox no longer holds is answered
    /// nothing and left out (RFC 9051 §6.4.9), so a page never lists a
    /// message a head stated removed.
    fn list_uids(
        &mut self,
        uids: &[u32],
        uid_validity: u32,
        held: &Held<'_>,
    ) -> Result<(Vec<EnumEntry>, u64)> {
        let trusted = trusts(held, uid_validity);
        let (heldset, unheld): (Vec<u32>, Vec<u32>) = uids
            .iter()
            .partition(|uid| trusted && held.holds(&uid.to_string()));

        let mut rows = self.fetch_spines(&heldset)?;
        let (metas, bytes) = self.fetch_metas(&unheld)?;
        rows.extend(metas.iter().map(|(uid, meta)| Spine {
            uid: *uid,
            flags: meta.flags.clone(),
        }));
        Ok((entries(rows, metas), bytes))
    }

    /// The UIDs of a round, highest first: every message, or the ones
    /// `SENTSINCE` and `SENTBEFORE` keep with a day's margin around the
    /// scope (the server compares dates, not instants, in its own zone) and
    /// the ones with no `Date`, above `above` and below `below` when given.
    /// None when the `SELECT` stated the mailbox `empty`.
    fn search_round(
        &mut self,
        scope: &PimdirScope,
        above: Option<u32>,
        below: Option<u32>,
        empty: bool,
    ) -> Result<Vec<u32>> {
        if empty {
            return Ok(Vec::new());
        }

        let mut criteria = Vec::new();
        if let Some(below) = below {
            if below <= 1 {
                return Ok(Vec::new());
            }
            let range: SequenceSet = format!("1:{}", below - 1)
                .as_str()
                .try_into()
                .map_err(|_| anyhow!("Invalid IMAP UID range below {below}"))?;
            criteria.push(SearchKey::Uid(range));
        }
        if let Some(above) = above.filter(|above| *above > 0) {
            let Some(from) = above.checked_add(1) else {
                return Ok(Vec::new());
            };
            // NOTE: `n:*` matches the highest UID even below `n` (RFC 9051
            // §9), which the filter below drops.
            let range: SequenceSet = format!("{from}:*")
                .as_str()
                .try_into()
                .map_err(|_| anyhow!("Invalid IMAP UID range above {above}"))?;
            criteria.push(SearchKey::Uid(range));
        }
        // NOTE: a message with no `Date` is in every scope, and `SENTSINCE`
        // matches none: it is searched for apart.
        let undated = || {
            let field = AString::try_from("Date").map_err(|_| anyhow!("Invalid IMAP field"))?;
            let empty = AString::try_from("").map_err(|_| anyhow!("Invalid IMAP value"))?;
            Ok::<_, anyhow::Error>(SearchKey::Not(Box::new(SearchKey::Header(field, empty))))
        };
        if let Some(day) = scope.since.as_deref().and_then(|since| imap_day(since, -1)) {
            criteria.push(SearchKey::Or(
                Box::new(SearchKey::SentSince(day)),
                Box::new(undated()?),
            ));
        }
        if let Some(day) = scope.until.as_deref().and_then(|until| imap_day(until, 2)) {
            criteria.push(SearchKey::Or(
                Box::new(SearchKey::SentBefore(day)),
                Box::new(undated()?),
            ));
        }
        if criteria.is_empty() {
            criteria.push(SearchKey::All);
        }
        let criteria = Vec1::try_from(criteria).map_err(|_| anyhow!("Empty IMAP search"))?;

        let found = self.search(criteria, ImapMessageSearchOptions { uid: true })?;
        let mut uids: Vec<u32> = rows_of("SEARCH", found)?
            .into_iter()
            .map(NonZeroU32::get)
            .filter(|uid| above.is_none_or(|above| *uid > above))
            .collect();
        uids.sort_unstable_by(|a, b| b.cmp(a));
        uids.dedup();
        Ok(uids)
    }

    /// The flags of a UID set, by `UID FETCH (UID FLAGS)`.
    fn fetch_spines(&mut self, uids: &[u32]) -> Result<Vec<Spine>> {
        let mut rows = Vec::with_capacity(uids.len());
        for chunk in uids.chunks(PAGE_SIZE) {
            let Some(set) = uid_set(chunk)? else {
                continue;
            };
            let fetched = self.fetch(
                set,
                uid_flag_names(),
                ImapMessageFetchOptions {
                    uid: true,
                    ..Default::default()
                },
            )?;
            rows.extend(
                rows_of("FETCH", fetched)?
                    .into_values()
                    .filter_map(|items| spine(&items.into_inner())),
            );
        }
        Ok(rows)
    }

    /// What names each of a UID set, read in one `UID FETCH (UID FLAGS
    /// RFC822.SIZE BODY.PEEK[HEADER.FIELDS (…)])` a page: the header fields
    /// Annex A reads, `Content-Type` for the attachment mark, and no
    /// `BODYSTRUCTURE`. Answers the metas by UID and the header octets read.
    fn fetch_metas(&mut self, uids: &[u32]) -> Result<(BTreeMap<u32, Meta>, u64)> {
        let mut metas = BTreeMap::new();
        let mut bytes = 0;
        for chunk in uids.chunks(PAGE_SIZE) {
            let Some(set) = uid_set(chunk)? else {
                continue;
            };
            let fetched = self.fetch(
                set,
                meta_names()?,
                ImapMessageFetchOptions {
                    uid: true,
                    ..Default::default()
                },
            )?;
            for items in rows_of("FETCH", fetched)?.into_values() {
                let Some((uid, meta, read)) = meta_from(items.into_inner()) else {
                    continue;
                };
                bytes += read;
                metas.insert(uid, meta);
            }
        }
        Ok((metas, bytes))
    }

    /// SELECTs `mailbox` unless it is already selected, so a run of fetches on
    /// one mailbox pays a single SELECT.
    fn select_cached(&mut self, mailbox: &str) -> Result<()> {
        if self.is_selected(mailbox) {
            return Ok(());
        }
        let mbox = parse_mailbox(mailbox)?;
        let select = self.select(mbox, ImapMailboxSelectOptions::default())?;
        data_of("SELECT", select);
        self.mark_selected(mailbox);
        Ok(())
    }

    /// What names each of a UID set, read off the header fields a listing
    /// reads ([`meta_names`]), never off an `ENVELOPE`, the server's own
    /// parse: a member is named one way whichever request reads it (pimdir
    /// STORAGE Annex A.1). Targeted, never a whole-mailbox `1:*` sweep.
    pub fn fetch_summaries(
        &mut self,
        mailbox: &str,
        uids: &[&str],
    ) -> Result<Vec<(String, PimdirDerivation)>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let uids = uids
            .iter()
            .map(|uid| {
                uid.parse::<NonZeroU32>()
                    .map(NonZeroU32::get)
                    .map_err(|_| anyhow!("Invalid message UID {uid}"))
            })
            .collect::<Result<Vec<u32>>>()?;

        self.select_cached(mailbox)?;
        let (metas, _) = self.fetch_metas(&uids)?;

        Ok(metas
            .into_iter()
            .map(|(uid, meta)| (uid.to_string(), meta.derivation))
            .collect())
    }

    /// Adds, sets, or removes `flags` on a UID set in `mailbox`.
    pub fn store_flags(
        &mut self,
        mailbox: &str,
        ids: &[&str],
        flags: &[Flag],
        op: FlagOp,
    ) -> Result<()> {
        let sequence_set = parse_uids(ids)?;
        let imap_flags: Vec<ImapFlag<'static>> = flags.iter().map(flag_from).collect();
        let kind = match op {
            FlagOp::Add => StoreType::Add,
            FlagOp::Remove => StoreType::Remove,
            FlagOp::Set => StoreType::Replace,
        };

        self.select_cached(mailbox)?;
        self.store(
            sequence_set,
            kind,
            imap_flags,
            ImapMessageStoreOptions { uid: true },
        )?;

        Ok(())
    }

    /// Adds `add` to and removes `remove` from a UID set's flags in
    /// `mailbox`, a `+FLAGS` then a `-FLAGS` store, each skipped when it
    /// names nothing: a flag named in neither stays as the server holds it.
    pub fn update_flags(
        &mut self,
        mailbox: &str,
        ids: &[&str],
        add: &[Flag],
        remove: &[Flag],
    ) -> Result<()> {
        if !add.is_empty() {
            self.store_flags(mailbox, ids, add, FlagOp::Add)?;
        }
        if !remove.is_empty() {
            self.store_flags(mailbox, ids, remove, FlagOp::Remove)?;
        }
        Ok(())
    }

    /// Streams a UID set's bodies in one batched `UID FETCH … (BODY.PEEK[])`,
    /// and answers the UIDs the server answered nothing for: a `UID FETCH`
    /// ignores a UID the mailbox no longer holds (RFC 9051 §6.4.9), so a
    /// command that completes leaves out exactly the messages gone.
    ///
    /// One SELECT and one FETCH for the whole set, and no body lands in memory
    /// whole. Named apart from the inner `fetch_bodies_stream` reached through
    /// `Deref`, which it would otherwise recurse into.
    pub fn fetch_bodies<S: Write>(
        &mut self,
        mailbox: &str,
        ids: &[&str],
        mut open: impl FnMut(&str) -> std::io::Result<S>,
        mut done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<Vec<String>> {
        let sequence_set = parse_uids(ids)?;
        self.select_cached(mailbox)?;
        let mut answered = BTreeSet::new();
        self.fetch_bodies_stream(
            sequence_set,
            true,
            |uid| open(&uid.to_string()),
            |uid, sink| {
                answered.insert(uid);
                done(&uid.to_string(), None, sink)
            },
        )
        .with_context(|| format!("Batched body fetch {mailbox} error"))?;
        Ok(ids
            .iter()
            .filter(|id| id.parse::<u32>().is_ok_and(|uid| !answered.contains(&uid)))
            .map(|id| id.to_string())
            .collect())
    }

    /// Leaves the connection fit for the next command after a batched body
    /// fetch failed with `err`: kept after a tagged `NO` or `BAD`, whose
    /// answer was read whole, opened again after any other failure.
    pub fn recover(&mut self, err: &anyhow::Error) -> Result<()> {
        let imap = err
            .chain()
            .find_map(|cause| cause.downcast_ref::<ImapClientError>());
        if imap.is_some_and(|err| err.status().is_some()) {
            return Ok(());
        }

        match imap.is_some_and(ImapClientError::is_connection_lost) {
            true => debug!("imap connection lost, opening it again"),
            false => debug!("imap answer left unread, opening the connection again"),
        }
        self.reconnect()
            .context("Cannot open the IMAP connection again")
    }

    /// Streams one message's raw RFC 5322 bytes into `sink` without flipping
    /// `\Seen` (BODY.PEEK[]); a UID answered with nothing is [`Gone`].
    pub fn get_message_stream(&mut self, mailbox: &str, id: &str, sink: impl Write) -> Result<()> {
        let uid: NonZeroU32 = id.parse().map_err(|_| anyhow!("Invalid IMAP UID {id}"))?;

        self.select_cached(mailbox)?;
        let mut sink = Counted { sink, octets: 0 };
        self.fetch_body_stream(uid, true, &mut sink)?;
        if sink.octets == 0 {
            return Err(anyhow::Error::new(Gone(format!("UID {id} in {mailbox}"))));
        }
        Ok(())
    }

    /// Appends `len` octets streamed from `source` to `mailbox` with `flags`.
    ///
    /// Returns the appended UID from the UIDPLUS APPENDUID response, falling
    /// back to a UID SEARCH on `message_id` where the server lacks UIDPLUS.
    /// `None` when the server took the message without saying which it is
    /// and the search names none: the append is accepted all the same
    /// (pimdir SYNC §4), and the next listing names the member.
    pub fn add_message_stream(
        &mut self,
        mailbox: &str,
        flags: &[Flag],
        source: impl Read,
        len: usize,
        message_id: Option<&str>,
    ) -> Result<Option<String>> {
        let mbox = parse_mailbox(mailbox)?;
        let imap_flags: Vec<ImapFlag<'static>> = flags.iter().map(flag_from).collect();

        let (_, appenduid) = self.append_stream(
            mbox.clone(),
            source,
            len,
            ImapMessageAppendOptions {
                flags: imap_flags,
                date: None,
                non_sync: false,
            },
        )?;

        if let Some((_, uid)) = appenduid {
            return Ok(Some(uid.to_string()));
        }

        let message_id = message_id.map(str::trim).filter(|id| !id.is_empty());
        let Some(message_id) = message_id else {
            debug!("append to {mailbox} answered no UID and states no Message-ID");
            return Ok(None);
        };

        self.select_cached(mailbox)?;

        let field =
            AString::try_from("Message-ID").map_err(|_| anyhow!("Invalid IMAP search header"))?;
        let value = AString::try_from(message_id.to_string())
            .map_err(|_| anyhow!("Invalid IMAP search Message-ID value"))?;
        let criteria = Vec1::from(SearchKey::Header(field, value));
        let found = self.search(criteria, ImapMessageSearchOptions { uid: true })?;
        let uids = data_of("SEARCH", found);

        let uid = uids.into_iter().max().map(|uid| uid.to_string());
        if uid.is_none() {
            debug!("append to {mailbox} answered no UID and a search finds none");
        }
        Ok(uid)
    }

    /// The UID of a message of `mailbox` stating `message_id` (bare, without
    /// its angle brackets), `None` when it holds none.
    pub fn find_message(&mut self, mailbox: &str, message_id: &str) -> Result<Option<String>> {
        self.select_cached(mailbox)?;

        // NOTE: HEADER matches substrings: brackets keep `a@x` off `<a@x.y>`.
        let field =
            AString::try_from("Message-ID").map_err(|_| anyhow!("Invalid IMAP search header"))?;
        let value = AString::try_from(format!("<{message_id}>"))
            .map_err(|_| anyhow!("Invalid IMAP search Message-ID value"))?;
        let criteria = Vec1::from(SearchKey::Header(field, value));
        let found = self.search(criteria, ImapMessageSearchOptions { uid: true })?;
        let uids = rows_of("SEARCH", found)?;

        Ok(uids.into_iter().min().map(|uid| uid.to_string()))
    }

    /// Copies a UID set from `from` to `to`.
    #[allow(dead_code)]
    pub fn copy_messages(&mut self, from: &str, to: &str, ids: &[&str]) -> Result<()> {
        let target = parse_mailbox(to)?;
        let sequence_set = parse_uids(ids)?;

        self.select_cached(from)?;
        self.copy(sequence_set, target, ImapMessageCopyOptions { uid: true })?;

        Ok(())
    }

    /// Moves a UID set from `from` to `to` (RFC 6851).
    pub fn move_messages(&mut self, from: &str, to: &str, ids: &[&str]) -> Result<()> {
        let target = parse_mailbox(to)?;
        let sequence_set = parse_uids(ids)?;

        self.select_cached(from)?;
        self.r#move(sequence_set, target, ImapMessageMoveOptions { uid: true })?;

        Ok(())
    }

    /// The name a mailbox `name` takes under `parent`: the two joined by
    /// the hierarchy delimiter the parent's `LIST` row states. A server
    /// with a flat namespace (no delimiter) nests nothing.
    pub fn child_mailbox(&mut self, parent: &str, name: &str) -> Result<String> {
        let reference: ImapMailbox<'static> = ""
            .try_into()
            .map_err(|_| anyhow!("Invalid IMAP list reference"))?;
        let pattern: ListMailbox<'static> = String::from(parent)
            .try_into()
            .map_err(|_| anyhow!("Invalid IMAP mailbox {parent}"))?;

        let rows = data_of("LIST", self.list(reference, pattern)?);
        let Some((_, delimiter, attributes)) = rows.first() else {
            return Err(anyhow::Error::new(Refusal(format!(
                "No IMAP mailbox {parent} to create {name} under"
            ))));
        };
        if attributes.contains(&FlagNameAttribute::Noinferiors) {
            return Err(anyhow::Error::new(Refusal(format!(
                "IMAP mailbox {parent} takes no child mailbox"
            ))));
        }
        let Some(delimiter) = delimiter else {
            return Err(anyhow::Error::new(Refusal(format!(
                "The IMAP server has a flat namespace, {name} cannot go under {parent}"
            ))));
        };
        let delimiter = delimiter.inner();
        if name.contains(delimiter) {
            return Err(anyhow::Error::new(Refusal(format!(
                "A mailbox name holds no {delimiter}, the server's hierarchy delimiter"
            ))));
        }

        Ok(format!("{parent}{delimiter}{name}"))
    }

    /// Creates a mailbox.
    pub fn create_mailbox(&mut self, mailbox: &str) -> Result<()> {
        let mbox = parse_mailbox(mailbox)?;
        self.create(mbox)?;
        Ok(())
    }

    /// Deletes a mailbox.
    pub fn delete_mailbox(&mut self, mailbox: &str) -> Result<()> {
        let mbox = parse_mailbox(mailbox)?;
        self.delete(mbox)?;
        Ok(())
    }

    /// Every mailbox the server lists, `\Noselect` ones included, by name,
    /// with the hierarchy delimiter its row states.
    fn mailbox_tree(&mut self) -> Result<Vec<(String, Option<char>)>> {
        let reference: ImapMailbox<'static> = ""
            .try_into()
            .map_err(|_| anyhow!("Invalid IMAP list reference"))?;
        let pattern: ListMailbox<'static> = "*"
            .try_into()
            .map_err(|_| anyhow!("Invalid IMAP list pattern"))?;

        Ok(data_of("LIST", self.list(reference, pattern)?)
            .into_iter()
            .map(|row| {
                let delimiter = row.1.as_ref().map(|delimiter| delimiter.inner());
                (mailbox_from(row).id, delimiter)
            })
            .collect())
    }

    /// Renames `mailbox` to `name` under the parent it has, the mailboxes
    /// under it following (RFC 9051 §6.3.6), and answers what it is then
    /// (pimdir Annex B.2 `collection-rename`).
    ///
    /// One already so named, or gone where the new name stands, is success;
    /// `INBOX`, a sibling already named `name`, a name holding the delimiter
    /// and a mailbox gone with nothing in its place are refused.
    pub fn rename_named_mailbox(&mut self, mailbox: &str, name: &str) -> Result<Renamed> {
        let refused = |why: String| Err(anyhow::Error::new(Refusal(why)));

        if mailbox.eq_ignore_ascii_case("INBOX") {
            return refused(String::from("The IMAP INBOX keeps its name"));
        }
        let tree = self.mailbox_tree()?;
        let listed = |id: &str| tree.iter().find(|(listed, _)| listed == id);
        let delimiter = listed(mailbox)
            .or_else(|| tree.first())
            .and_then(|(_, delimiter)| *delimiter);
        let target = renamed_mailbox(mailbox, name, delimiter)?;
        let renamed = Renamed {
            id: target.clone(),
            name: target.clone(),
            delimiter,
        };

        match (listed(mailbox).is_some(), listed(&target).is_some()) {
            _ if target == mailbox => Ok(renamed),
            (true, true) => refused(format!("IMAP mailbox {target} already exists")),
            (false, true) => Ok(renamed),
            (false, false) => refused(format!("No IMAP mailbox {mailbox} to rename")),
            (true, false) => {
                self.rename(parse_mailbox(mailbox)?, parse_mailbox(&target)?)?;
                Ok(renamed)
            }
        }
    }

    /// Deletes `mailbox` with its messages, unless a mailbox stands under it
    /// (pimdir Annex B.2 `collection-delete`): a server deleting it then
    /// keeps it as a `\Noselect` container (RFC 9051 §6.3.5), half done.
    ///
    /// One already gone is success; `INBOX` is refused.
    pub fn delete_named_mailbox(&mut self, mailbox: &str) -> Result<()> {
        if mailbox.eq_ignore_ascii_case("INBOX") {
            return Err(anyhow::Error::new(Refusal(String::from(
                "The IMAP INBOX cannot be deleted",
            ))));
        }
        let tree = self.mailbox_tree()?;
        let Some((_, delimiter)) = tree.iter().find(|(listed, _)| listed == mailbox) else {
            return Ok(());
        };
        let children = children_of(&tree, mailbox, *delimiter);
        if !children.is_empty() {
            return Err(anyhow::Error::new(Refusal(format!(
                "IMAP mailbox {mailbox} holds {}: delete them first",
                children.join(", ")
            ))));
        }

        self.delete_mailbox(mailbox)
    }

    /// Deletes one message: marks it `\Deleted`, then `UID EXPUNGE`s that
    /// UID alone (RFC 4315, built into IMAP4rev2).
    ///
    /// A plain `EXPUNGE` would remove every message any client marked
    /// `\Deleted` in the mailbox, so a server offering neither UIDPLUS nor
    /// IMAP4rev2 gets no delete at all: the push fails, io-pimdir keeps it,
    /// and no `\Deleted` flag is left behind, the capability being read
    /// before anything is stored.
    pub fn delete_message(&mut self, mailbox: &str, id: &str) -> Result<()> {
        if !self.supports_uid_expunge() {
            bail!(
                "The IMAP server offers neither UIDPLUS nor IMAP4rev2, so a single message \
                 cannot be expunged without expunging every message marked \\Deleted in \
                 {mailbox}: delete refused"
            );
        }

        let sequence_set = parse_uids(&[id])?;

        self.select_cached(mailbox)?;
        self.store(
            sequence_set.clone(),
            StoreType::Add,
            vec![ImapFlag::Deleted],
            ImapMessageStoreOptions { uid: true },
        )?;
        self.uid_expunge(sequence_set)?;

        Ok(())
    }
}

/// The name `mailbox` takes renamed `name`: under the parent it has, joined
/// by `delimiter`, at the top on a flat namespace. A blank name, or one
/// holding the delimiter, which would nest by itself, is refused.
fn renamed_mailbox(mailbox: &str, name: &str, delimiter: Option<char>) -> Result<String> {
    if name.trim().is_empty() || delimiter.is_some_and(|delimiter| name.contains(delimiter)) {
        return Err(anyhow::Error::new(Refusal(format!(
            "An IMAP mailbox name is not blank and holds no hierarchy delimiter: {name:?}"
        ))));
    }

    Ok(
        match delimiter.and_then(|delimiter| Some((delimiter, mailbox.rsplit_once(delimiter)?))) {
            Some((delimiter, (parent, _))) => format!("{parent}{delimiter}{name}"),
            None => name.to_owned(),
        },
    )
}

/// The mailboxes of `tree` standing under `mailbox`, at any depth.
fn children_of(
    tree: &[(String, Option<char>)],
    mailbox: &str,
    delimiter: Option<char>,
) -> Vec<String> {
    let Some(delimiter) = delimiter else {
        return Vec::new();
    };
    let under = format!("{mailbox}{delimiter}");
    tree.iter()
        .filter(|(id, _)| id.starts_with(&under))
        .map(|(id, _)| id.clone())
        .collect()
}

/// One IMAP LIST row (mailbox, delimiter, attributes).
type ListRow = (
    ImapMailbox<'static>,
    Option<QuotedChar>,
    Vec<FlagNameAttribute<'static>>,
);

/// Drops `\Noselect` containers (RFC 3501 §6.3.8), which cannot hold messages.
fn is_selectable(row: &ListRow) -> bool {
    !row.2.contains(&FlagNameAttribute::Noselect)
}

/// The role a LIST row states: `INBOX`, or its first SPECIAL-USE attribute
/// (RFC 6154). An attribute neverest does not know states nothing.
fn role_of(row: &ListRow) -> Option<&'static str> {
    if matches!(row.0, ImapMailbox::Inbox) {
        return Some("inbox");
    }

    row.2.iter().find_map(
        |attribute| match attribute.to_string().to_ascii_lowercase().as_str() {
            "\\sent" => Some("sent"),
            "\\drafts" => Some("drafts"),
            "\\trash" => Some("trash"),
            "\\junk" => Some("junk"),
            "\\all" => Some("all"),
            "\\archive" => Some("archive"),
            "\\flagged" => Some("flagged"),
            "\\important" => Some("important"),
            _ => None,
        },
    )
}

/// Converts one IMAP LIST row into the shared [`Collection`] shape.
fn mailbox_from(row: ListRow) -> Collection {
    let role = role_of(&row).map(String::from);
    let name = match row.0 {
        ImapMailbox::Inbox => "INBOX".to_string(),
        ImapMailbox::Other(other) => String::from_utf8_lossy(other.inner().as_ref()).into_owned(),
    };

    Collection {
        id: name.clone(),
        name,
        total: None,
        unread: None,
        role,
        takes_tasks: None,
    }
}

/// Folds a STATUS response into the matching mailbox row.
fn apply_status(mailbox: &mut Collection, items: Vec<StatusDataItem>) {
    for item in items {
        match item {
            StatusDataItem::Messages(n) => mailbox.total = Some(u64::from(n)),
            StatusDataItem::Unseen(n) => mailbox.unread = Some(u64::from(n)),
            _ => {}
        }
    }
}

/// How many UIDs one page of a round lists (pimdir SYNC §4): the flags,
/// the size and a few header fields are under a kilobyte a message, half a
/// megabyte a response, short enough to resume cheaply.
pub(crate) const PAGE_SIZE: usize = 500;

/// The header fields a listing reads to name a message (pimdir STORAGE
/// Annex A.1), `Content-Type` for the attachment mark without the body.
const META_FIELDS: [&str; 9] = [
    "Date",
    "From",
    "To",
    "Cc",
    "Bcc",
    "Subject",
    "Message-ID",
    "In-Reply-To",
    "Content-Type",
];

/// One listed message: its UID and its flags.
struct Spine {
    uid: u32,
    flags: BTreeSet<Flag>,
}

/// What names one listed message, with its flags.
struct Meta {
    flags: BTreeSet<Flag>,
    derivation: PimdirDerivation,
}

/// The result of `command`, warning of the server's alerts and of the
/// response lines skipped for not decoding.
fn data_of<T>(command: &str, output: ImapOutput<T>) -> T {
    for alert in &output.alerts {
        warn!("server sent an alert: {alert}");
    }

    if !output.skipped.is_empty() {
        let count = output.skipped.len();
        warn!("skipped {count} unreadable line(s) of the {command} response");
    }

    output.data
}

/// The rows a listing `command` answered, refused when a line of the
/// response was skipped for not decoding: read short, it states removals.
fn rows_of<T>(command: &str, output: ImapOutput<T>) -> Result<T> {
    for alert in &output.alerts {
        warn!("server sent an alert: {alert}");
    }

    if !output.skipped.is_empty() {
        let count = output.skipped.len();
        bail!("IMAP {command} response holds {count} line(s) that do not decode");
    }

    Ok(output.data)
}

/// The lean FETCH item set for enumeration: UID + FLAGS only (no ENVELOPE).
fn uid_flag_names() -> MacroOrMessageDataItemNames<'static> {
    MacroOrMessageDataItemNames::MessageDataItemNames(vec![
        MessageDataItemName::Uid,
        MessageDataItemName::Flags,
    ])
}

/// The FETCH item set naming a message: UID, FLAGS, RFC822.SIZE and the
/// [`META_FIELDS`], peeked so `\Seen` stays as it is.
fn meta_names() -> Result<MacroOrMessageDataItemNames<'static>> {
    let fields = META_FIELDS
        .iter()
        .map(|field| AString::try_from(*field).map_err(|_| anyhow!("Invalid IMAP field {field}")))
        .collect::<Result<Vec<_>>>()?;
    let fields = Vec1::try_from(fields).map_err(|_| anyhow!("Empty IMAP header field list"))?;

    Ok(MacroOrMessageDataItemNames::MessageDataItemNames(vec![
        MessageDataItemName::Uid,
        MessageDataItemName::Flags,
        MessageDataItemName::Rfc822Size,
        MessageDataItemName::BodyExt {
            section: Some(Section::HeaderFields(None, fields)),
            partial: None,
            peek: true,
        },
    ]))
}

/// The UID and flags of one FETCH row; `None` without a UID.
fn spine(items: &[MessageDataItem<'static>]) -> Option<Spine> {
    let mut uid = None;
    let mut flags = BTreeSet::new();
    for item in items {
        match item {
            MessageDataItem::Uid(u) => uid = Some(u.get()),
            MessageDataItem::Flags(fs) => {
                flags = fs.iter().cloned().filter_map(flag_from_fetch).collect();
            }
            _ => {}
        }
    }
    Some(Spine { uid: uid?, flags })
}

/// What one FETCH row of [`meta_names`] names, with the header octets it
/// carried; `None` without a UID.
///
/// The header block is read the way io-pimdir reads a whole message, so a
/// message named here and later hydrated derives the same key and summary.
fn meta_from(items: Vec<MessageDataItem<'static>>) -> Option<(u32, Meta, u64)> {
    let mut uid = None;
    let mut flags = BTreeSet::new();
    let mut size = None;
    let mut header = Vec::new();

    for item in items {
        match item {
            MessageDataItem::Uid(u) => uid = Some(u.get()),
            MessageDataItem::Flags(fs) => {
                flags = fs.into_iter().filter_map(flag_from_fetch).collect();
            }
            MessageDataItem::Rfc822Size(n) => size = Some(u64::from(n)),
            MessageDataItem::BodyExt { data, .. } => {
                if let Some(bytes) = data.into_option() {
                    header = bytes.into_owned();
                }
            }
            _ => {}
        }
    }

    let read = header.len() as u64;
    let derivation = mail::derive_meta(&header, size.filter(|size| *size > 0), None);
    Some((uid?, Meta { flags, derivation }, read))
}

/// The entries of listed rows, each named by its meta when one was read.
fn entries(rows: Vec<Spine>, mut metas: BTreeMap<u32, Meta>) -> Vec<EnumEntry> {
    rows.into_iter()
        .map(|row| EnumEntry {
            id: row.uid.to_string(),
            flags: row.flags,
            revision: None,
            meta: metas.remove(&row.uid).map(|meta| meta.derivation),
        })
        .collect()
}

/// A UID set from UIDs, `None` when empty.
fn uid_set(uids: &[u32]) -> Result<Option<SequenceSet>> {
    let uids: Vec<NonZeroU32> = uids
        .iter()
        .filter_map(|uid| NonZeroU32::new(*uid))
        .collect();
    if uids.is_empty() {
        return Ok(None);
    }
    SequenceSet::try_from(uids)
        .map(Some)
        .map_err(|_| anyhow!("Invalid UID set"))
}

/// Whether a round's search over `scope` is narrowed by a date filter
/// (`SENTSINCE` or `SENTBEFORE`), as [`ImapClient::search_round`] builds it.
fn narrows(scope: &PimdirScope) -> bool {
    let since = scope.since.as_deref().and_then(|at| imap_day(at, -1));
    let until = scope.until.as_deref().and_then(|at| imap_day(at, 2));
    since.is_some() || until.is_some()
}

/// The day an RFC 3339 instant falls on in UTC, moved by `shift` days, as
/// an IMAP search date.
fn imap_day(instant: &str, shift: i64) -> Option<ImapDate> {
    let day = DateTime::parse_from_rfc3339(instant)
        .ok()?
        .to_utc()
        .date_naive();
    let day = day.checked_add_signed(chrono::Duration::days(shift))?;
    ImapDate::try_from(day).ok()
}

/// Where a round stands: the `UIDVALIDITY` it lists under, the lowest UID
/// it listed, and the highest UID it found when it opened, above which
/// every UID arrived since.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RoundCursor {
    uid_validity: u32,
    lowest: u32,
    top: u32,
}

/// Encodes a round's resume cursor: little-endian, 4 bytes each of the
/// `UIDVALIDITY`, the lowest UID listed and the highest UID found.
fn encode_cursor(cursor: RoundCursor) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(&cursor.uid_validity.to_le_bytes());
    bytes.extend_from_slice(&cursor.lowest.to_le_bytes());
    bytes.extend_from_slice(&cursor.top.to_le_bytes());
    bytes
}

/// Decodes a round's resume cursor; `None` for bytes that are not one.
///
/// A cursor written before it carried the highest UID found reads it as 0:
/// the round's last page then lists its whole scope again, which keeps
/// whatever arrived since, at the price of one pass over the flags.
fn decode_cursor(bytes: &[u8]) -> Option<RoundCursor> {
    let word = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    match bytes.len() {
        8 | 12 => Some(RoundCursor {
            uid_validity: word(0)?,
            lowest: word(4)?,
            top: if bytes.len() == 12 { word(8)? } else { 0 },
        }),
        _ => None,
    }
}

/// The `(UIDVALIDITY, modseq)` a QRESYNC `SELECT` lists from: `None` for a
/// server without QRESYNC, bytes that are no checkpoint, or a checkpoint
/// with no `UIDVALIDITY` or no modseq (a mailbox without `CONDSTORE`).
fn qresync_position(checkpoint: &[u8], qresync: bool) -> Option<(NonZeroU32, u64)> {
    let (validity, modseq) = decode_checkpoint(checkpoint)?;
    let validity = NonZeroU32::new(validity)?;
    (qresync && modseq > 0).then_some((validity, modseq))
}

/// The handles the store binds among the `VANISHED (EARLIER)` UID ranges.
///
/// A handle the store does not bind names nothing to remove, and the ranges
/// are never expanded: a server may announce `1:4294967295`.
fn held_vanished(ranges: &[RangeInclusive<NonZeroU32>], held: &Held<'_>) -> Vec<String> {
    let uids: BTreeMap<u32, &String> = held
        .handles
        .iter()
        .filter_map(|handle| Some((handle.parse().ok()?, handle)))
        .collect();

    // NOTE: ranges may overlap, the set keeps each handle once.
    let vanished: BTreeSet<&String> = ranges
        .iter()
        .flat_map(|range| uids.range(range.start().get()..=range.end().get()))
        .map(|(_, handle)| *handle)
        .collect();

    vanished.into_iter().cloned().collect()
}

/// Whether the store's handles name what `uid_validity` numbers: a handle
/// names what the store binds only under the `UIDVALIDITY` its checkpoint
/// was made under.
fn trusts(held: &Held<'_>, uid_validity: u32) -> bool {
    held.checkpoint
        .and_then(checkpoint_uid_validity)
        .is_some_and(|validity| validity == uid_validity)
}

/// Whether the store bound its handles under another `UIDVALIDITY` than
/// `uid_validity`, every one of them then void (pimdir SYNC §4, §8). A store
/// holding no checkpoint, a rekey's own listing among them, bound none it
/// could tell.
fn renumbered(held: &Held<'_>, uid_validity: u32) -> bool {
    held.checkpoint
        .and_then(checkpoint_uid_validity)
        .is_some_and(|validity| validity != uid_validity)
}

fn flag_from_fetch(fetch: FlagFetch<'_>) -> Option<Flag> {
    let FlagFetch::Flag(flag) = fetch else {
        return None;
    };
    Some(Flag::from_raw(flag.to_string()))
}

/// Maps a shared [`Flag`] to its IMAP wire counterpart: IANA flags become the
/// matching system flag, custom keywords pass through as Keyword atoms.
fn flag_from(flag: &Flag) -> ImapFlag<'static> {
    match flag.iana() {
        Some(IanaFlag::Seen) => ImapFlag::Seen,
        Some(IanaFlag::Answered) => ImapFlag::Answered,
        Some(IanaFlag::Flagged) => ImapFlag::Flagged,
        Some(IanaFlag::Draft) => ImapFlag::Draft,
        Some(IanaFlag::Deleted) => ImapFlag::Deleted,
        Some(_) => ImapFlag::keyword(
            Atom::try_from(String::from(flag.raw()))
                .expect("canonical IANA keyword is a valid IMAP atom"),
        ),
        None => match Atom::try_from(String::from(flag.raw())) {
            Ok(atom) => ImapFlag::keyword(atom),
            Err(_) => ImapFlag::keyword(
                Atom::try_from(sanitise_atom(flag.raw()))
                    .expect("sanitised atom contains only atom-safe ASCII"),
            ),
        },
    }
}

/// Replaces every non-atom-safe byte with `_` so a keyword with spaces,
/// controls or `()<>{}` survives IMAP STORE.
fn sanitise_atom(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii()
                && !c.is_control()
                && !matches!(
                    c,
                    ' ' | '(' | ')' | '{' | '%' | '*' | '"' | '\\' | ']' | '\x7f'
                )
            {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Parses a shared mailbox name into an IMAP Mailbox token.
fn parse_mailbox(name: &str) -> Result<ImapMailbox<'static>> {
    String::from(name)
        .try_into()
        .map_err(|_| anyhow!("Invalid IMAP mailbox {name}"))
}

/// A sink counting the octets written through it.
struct Counted<W> {
    sink: W,
    octets: u64,
}

impl<W: Write> Write for Counted<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.sink.write(buf)?;
        self.octets += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.sink.flush()
    }
}

/// Parses stringified UIDs into an IMAP [`SequenceSet`].
fn parse_uids(ids: &[&str]) -> Result<SequenceSet> {
    if ids.is_empty() {
        bail!("Empty UID set");
    }

    let uids: Vec<NonZeroU32> = ids
        .iter()
        .map(|s| {
            s.parse::<NonZeroU32>()
                .map_err(|_| anyhow!("Invalid message UID {s}"))
        })
        .collect::<Result<_>>()?;

    SequenceSet::try_from(uids).map_err(|_| anyhow!("Invalid UID set"))
}

/// Encodes an IMAP sync cursor `(UIDVALIDITY, HIGHESTMODSEQ)` into checkpoint
/// bytes: little-endian, a 4-byte uidvalidity then an 8-byte modseq.
pub(crate) fn encode_checkpoint(uid_validity: u32, highest_mod_seq: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(&uid_validity.to_le_bytes());
    bytes.extend_from_slice(&highest_mod_seq.to_le_bytes());
    bytes
}

/// Decodes an IMAP sync cursor; `None` for absent or malformed bytes (a
/// non-CONDSTORE checkpoint has `modseq = 0`, which forces a full enumerate).
pub(crate) fn decode_checkpoint(bytes: &[u8]) -> Option<(u32, u64)> {
    if bytes.len() != 12 {
        return None;
    }
    let uid_validity = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
    let highest_mod_seq = u64::from_le_bytes(bytes[4..12].try_into().ok()?);
    Some((uid_validity, highest_mod_seq))
}

/// The backend UIDVALIDITY an IMAP checkpoint carries, `None` when the bytes
/// are not an IMAP cursor.
///
/// The driver compares it before and after a pull to detect a handle-space
/// change, which rebuilds the collection and bumps its generation.
pub(crate) fn checkpoint_uid_validity(bytes: &[u8]) -> Option<u32> {
    decode_checkpoint(bytes).map(|(uid_validity, _)| uid_validity)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use io_pimdir::collection::{PimdirCheckpoint, PimdirCursor};
    use pimalaya_stream::tls::Tls;

    use crate::{
        imap::fake::{FakeImap, FakeMailbox, FakeMessage},
        kind::mail::vectors::VECTORS,
    };

    use super::*;

    #[test]
    fn a_renamed_mailbox_keeps_its_parent() {
        assert_eq!(
            renamed_mailbox("Projects/2025", "2026", Some('/')).unwrap(),
            "Projects/2026"
        );
        assert_eq!(renamed_mailbox("Old", "New", Some('.')).unwrap(), "New");
        assert_eq!(renamed_mailbox("A.B", "C", None).unwrap(), "C");
        assert!(renamed_mailbox("Old", "A/B", Some('/')).is_err());
        assert!(renamed_mailbox("Old", " ", Some('/')).is_err());
    }

    #[test]
    fn the_mailboxes_under_one_are_found_at_any_depth() {
        let tree = [
            ("A".to_string(), Some('/')),
            ("A/B".to_string(), Some('/')),
            ("A/B/C".to_string(), Some('/')),
            ("AB".to_string(), Some('/')),
        ];

        assert_eq!(children_of(&tree, "A", Some('/')), ["A/B", "A/B/C"]);
        assert!(children_of(&tree, "AB", Some('/')).is_empty());
        assert!(children_of(&tree, "A", None).is_empty());
    }

    /// A mailbox renumbered to `UIDVALIDITY` 2, its one message now UID 7.
    fn renumbered(qresync: bool) -> FakeImap {
        FakeImap::start(
            FakeMailbox {
                uid_validity: 2,
                highest_mod_seq: 9,
                messages: vec![FakeMessage {
                    uid: 7,
                    flags: Vec::new(),
                    message_id: String::from("a@x"),
                }],
                states_exists: true,
            },
            qresync,
        )
    }

    /// What `listing` answers on `server`, the store binding UID 1 under
    /// the checkpoint `held`.
    fn listed(server: &FakeImap, listing: PimdirListing, held: Option<&[u8]>) -> &'static str {
        let mut client = ImapClient::connect(&server.url, &Tls::default(), false, None).unwrap();
        let handles = HashSet::from([String::from("1")]);
        let held = Held {
            handles: &handles,
            checkpoint: held,
            coverage: None,
        };
        let request = PimdirEnumerate {
            listing,
            scope: PimdirScope::unbounded(),
        };
        match client.enumerate("INBOX", &request, held).unwrap() {
            Listed::Page(_) => "page",
            Listed::CursorRejected => "cursor rejected",
            Listed::NoHead => "no head",
            Listed::HeadRejected => "head rejected",
            Listed::Rekey => "rekey",
        }
    }

    /// A store checkpoint under `UIDVALIDITY` 1.
    fn stored() -> Vec<u8> {
        encode_checkpoint(1, 5)
    }

    /// A round cursor under `uid_validity`.
    fn cursor(uid_validity: u32) -> PimdirCursor {
        PimdirCursor(encode_cursor(RoundCursor {
            uid_validity,
            lowest: 3,
            top: 5,
        }))
    }

    /// Every listing meeting a moved `UIDVALIDITY` fails and names a rekey
    /// (pimdir SYNC §4), never answering a round of the new UIDs.
    #[test]
    fn a_moved_uidvalidity_names_a_rekey_on_every_listing() {
        let qresync = renumbered(true);
        let plain = renumbered(false);
        let checkpoint = || PimdirCheckpoint(stored());
        let round = |cursor| PimdirListing::Round {
            cursor,
            band: false,
        };

        let cases = [
            (
                "qresync delta",
                &qresync,
                PimdirListing::Delta(checkpoint()),
            ),
            ("plain delta", &plain, PimdirListing::Delta(checkpoint())),
            (
                "delta head",
                &qresync,
                PimdirListing::Head(PimdirHead::Delta(checkpoint())),
            ),
            (
                "band head",
                &plain,
                PimdirListing::Head(PimdirHead::Band {
                    since: String::from("2026-10-01T00:00:00Z"),
                }),
            ),
            ("first page", &plain, round(None)),
            ("resumed page", &plain, round(Some(cursor(1)))),
        ];
        let answers: Vec<_> = cases
            .into_iter()
            .map(|(case, server, listing)| (case, listed(server, listing, Some(&stored()))))
            .collect();
        let rekeys: Vec<_> = answers.iter().map(|(case, _)| (*case, "rekey")).collect();
        assert_eq!(answers, rekeys);
    }

    /// A round whose cursor predates a rekey the store already made
    /// restarts, and a rekey's own round, the store holding no checkpoint
    /// for it, lists the new UIDs.
    #[test]
    fn a_rekeyed_store_lists_the_new_uids() {
        let server = renumbered(false);
        let rekeyed = encode_checkpoint(2, 9);
        let round = |cursor| PimdirListing::Round {
            cursor,
            band: false,
        };

        assert_eq!(
            listed(&server, round(Some(cursor(1))), Some(&rekeyed)),
            "cursor rejected"
        );
        assert_eq!(listed(&server, round(None), Some(&rekeyed)), "page");
        assert_eq!(listed(&server, round(None), None), "page");
    }

    /// A `SELECT` that states no `EXISTS` says nothing of the mailbox's
    /// size: read as empty, a complete round would delete every message.
    #[test]
    fn a_select_without_exists_is_searched() {
        let server = FakeImap::start(
            FakeMailbox {
                uid_validity: 2,
                highest_mod_seq: 9,
                messages: vec![FakeMessage {
                    uid: 7,
                    flags: Vec::new(),
                    message_id: String::from("a@x"),
                }],
                states_exists: false,
            },
            false,
        );
        let listings = [
            PimdirListing::Round {
                cursor: None,
                band: false,
            },
            PimdirListing::Head(PimdirHead::Band {
                since: String::from("2026-10-01T00:00:00Z"),
            }),
        ];

        for listing in listings {
            let mut client =
                ImapClient::connect(&server.url, &Tls::default(), false, None).unwrap();
            let handles = HashSet::new();
            let held = Held {
                handles: &handles,
                checkpoint: None,
                coverage: None,
            };
            let request = PimdirEnumerate {
                listing,
                scope: PimdirScope::unbounded(),
            };
            let Listed::Page(page) = client.enumerate("INBOX", &request, held).unwrap() else {
                panic!("a page");
            };
            let uids: Vec<_> = page.items.iter().map(|item| item.id.as_str()).collect();
            assert_eq!(uids, ["7"], "{:?}", request.listing);
        }
    }

    /// A row of a listing that does not decode is no message gone: the
    /// page fails, where one read without the row would state it removed.
    #[test]
    fn a_listing_row_that_does_not_decode_fails_the_page() {
        let message = |uid, flags: &[&str]| FakeMessage {
            uid,
            flags: flags.iter().map(|flag| flag.to_string()).collect(),
            message_id: format!("m{uid}@x"),
        };
        let server = FakeImap::start(
            FakeMailbox {
                uid_validity: 2,
                highest_mod_seq: 9,
                // NOTE: `]` is no atom character (RFC 9051 §9).
                messages: vec![message(7, &[]), message(8, &["a]b"])],
                states_exists: true,
            },
            false,
        );
        let request = PimdirEnumerate {
            listing: PimdirListing::Round {
                cursor: None,
                band: false,
            },
            scope: PimdirScope::unbounded(),
        };

        for bound in [&[][..], &["7", "8"][..]] {
            let mut client =
                ImapClient::connect(&server.url, &Tls::default(), false, None).unwrap();
            let handles: HashSet<String> = bound.iter().map(|uid| uid.to_string()).collect();
            let held = Held {
                handles: &handles,
                checkpoint: Some(&encode_checkpoint(2, 9)),
                coverage: None,
            };

            let err = match client.enumerate("INBOX", &request, held) {
                Ok(_) => panic!("a page without the message 8, bound {bound:?}"),
                Err(err) => err,
            };
            assert!(err.to_string().contains("do not decode"), "{err:#}");
        }
    }

    /// A round a scope narrows by `SENTSINCE` states it on its page, and
    /// its last page hands every UID by `UID SEARCH ALL` (pimdir SYNC §4,
    /// §5); a band round states it alone, and a round over the whole
    /// mailbox states neither.
    #[test]
    fn a_narrowed_round_hands_every_uid_on_its_last_page() {
        let server = FakeImap::start(
            FakeMailbox {
                uid_validity: 2,
                highest_mod_seq: 9,
                messages: ["a@x", "b@x"]
                    .into_iter()
                    .zip(1..)
                    .map(|(id, uid)| FakeMessage {
                        uid,
                        flags: Vec::new(),
                        message_id: String::from(id),
                    })
                    .collect(),
                states_exists: true,
            },
            false,
        );
        let since = PimdirScope::since("2026-09-01T00:00:00Z");
        let cases = [
            ("scoped", false, since.clone(), true, Some(vec!["1", "2"])),
            ("band", true, since, true, None),
            ("whole", false, PimdirScope::unbounded(), false, None),
        ];

        for (case, band, scope, narrowed, enumerated) in cases {
            let searches = server.sent("UID SEARCH").len();
            let mut client =
                ImapClient::connect(&server.url, &Tls::default(), false, None).unwrap();
            let handles = HashSet::new();
            let held = Held {
                handles: &handles,
                checkpoint: None,
                coverage: None,
            };
            let request = PimdirEnumerate {
                listing: PimdirListing::Round { cursor: None, band },
                scope,
            };
            let Listed::Page(page) = client.enumerate("INBOX", &request, held).unwrap() else {
                panic!("a page");
            };

            assert_eq!(page.narrowed, narrowed, "{case}");
            let enumerated: Option<Vec<String>> =
                enumerated.map(|uids| uids.into_iter().map(String::from).collect());
            assert_eq!(page.enumerated, enumerated, "{case}");
            let sent = server.sent("UID SEARCH");
            let all = sent[searches..]
                .iter()
                .filter(|line| line.eq_ignore_ascii_case("UID SEARCH ALL"))
                .count();
            // NOTE: the whole mailbox's round searches `ALL` itself, the
            // scoped one for its enumeration, the band one not at all.
            assert_eq!(all, usize::from(case != "band"), "{case}: {sent:?}");
        }
    }

    #[test]
    fn checkpoint_round_trips_and_rejects_garbage() {
        let bytes = encode_checkpoint(1_774_329_954, 5035);
        assert_eq!(decode_checkpoint(&bytes), Some((1_774_329_954, 5035)));
        assert_eq!(checkpoint_uid_validity(&bytes), Some(1_774_329_954));
        assert_eq!(decode_checkpoint(&[]), None);
        assert_eq!(decode_checkpoint(&[0; 8]), None);
        assert_eq!(checkpoint_uid_validity(&[0; 3]), None);
        assert_eq!(decode_checkpoint(&encode_checkpoint(42, 0)), Some((42, 0)));
    }

    fn row(name: &str, attributes: &[&str]) -> ListRow {
        let mailbox: ImapMailbox<'static> = name.to_owned().try_into().unwrap();
        let attributes = attributes
            .iter()
            .map(|attribute| {
                FlagNameAttribute::from(Atom::try_from(attribute.to_string()).unwrap())
            })
            .collect();
        (mailbox, None, attributes)
    }

    #[test]
    fn a_list_row_states_its_role_by_its_attributes_never_its_name() {
        assert_eq!(role_of(&row("INBOX", &[])), Some("inbox"));
        assert_eq!(role_of(&row("inbox", &[])), Some("inbox"));
        assert_eq!(
            role_of(&row("[Gmail]/Bin", &["HasNoChildren", "Trash"])),
            Some("trash")
        );
        assert_eq!(role_of(&row("[Gmail]/All Mail", &["All"])), Some("all"));
        assert_eq!(role_of(&row("Sent Items", &["SENT"])), Some("sent"));
        assert_eq!(role_of(&row("Drafts", &["Drafts"])), Some("drafts"));
        assert_eq!(role_of(&row("Spam", &["Junk"])), Some("junk"));
        assert_eq!(role_of(&row("Archives", &["Archive"])), Some("archive"));
        assert_eq!(role_of(&row("Sent", &[])), None);
        assert_eq!(
            role_of(&row("Important", &["Important"])),
            Some("important")
        );
        assert_eq!(role_of(&row("Starred", &["Flagged"])), Some("flagged"));
        assert_eq!(role_of(&row("Lists", &["Subscribed"])), None);
    }

    /// One FETCH row of [`meta_names`]: UID, flags, size and header block.
    fn meta_row(uid: u32, size: u32, header: &str) -> Vec<MessageDataItem<'static>> {
        use io_imap::types::core::{Literal, NString};

        let literal = Literal::try_from(header.as_bytes().to_vec()).unwrap();
        vec![
            MessageDataItem::Uid(NonZeroU32::new(uid).unwrap()),
            MessageDataItem::Flags(vec![FlagFetch::Flag(ImapFlag::Seen)]),
            MessageDataItem::Rfc822Size(size),
            MessageDataItem::BodyExt {
                section: None,
                origin: None,
                data: NString::from(literal),
            },
        ]
    }

    fn summary(derivation: &PimdirDerivation) -> &io_pimdir::summary::mail::PimdirMailSummary {
        match derivation.summary.as_ref() {
            Some(io_pimdir::summary::PimdirSummary::Mail(summary)) => summary,
            other => panic!("expected a mail summary, got {other:?}"),
        }
    }

    /// A listed message is named off its header fields, as its body would
    /// name it: the `Date` header (never the server's arrival), the size the
    /// server states, and the attachment mark from the top-level
    /// `Content-Type`, both ways, with no `BODYSTRUCTURE`.
    #[test]
    fn a_listed_message_is_named_by_its_header_fields() {
        let mixed = "Message-ID: <a@example.org>\r\n\
                     Date: Wed, 01 Jan 2020 10:00:00 +0100\r\n\
                     From: Alice <alice@example.org>\r\n\
                     Subject: Report\r\n\
                     Content-Type: multipart/mixed; boundary=x\r\n\r\n";
        let (uid, meta, read) = meta_from(meta_row(42, 9_000, mixed)).unwrap();

        assert_eq!(uid, 42);
        assert_eq!(read, mixed.len() as u64, "the header octets are counted");
        assert!(meta.flags.contains(&Flag::from_iana(IanaFlag::Seen)));
        assert_eq!(meta.derivation.link_id.as_str(), "a@example.org");
        let named = summary(&meta.derivation);
        assert_eq!(named.date.as_deref(), Some("2020-01-01T09:00:00Z"));
        assert_eq!(meta.derivation.sort_key.as_str(), "2020-01-01T09:00:00Z");
        assert_eq!(named.size, Some(9_000));
        assert_eq!(named.attachment, Some(true));
        assert_eq!(named.sender.as_deref(), Some("alice@example.org"));

        let plain = "Message-ID: <b@example.org>\r\nContent-Type: text/plain\r\n\r\n";
        let (_, meta, _) = meta_from(meta_row(43, 10, plain)).unwrap();
        let named = summary(&meta.derivation);
        assert_eq!(named.attachment, Some(false));
        assert_eq!(named.date, None, "no Date header, no date");
    }

    /// A message fetched at the `Meta` tier is named as its body would name
    /// it (pimdir STORAGE Annex A.1, vectors/summaries.json): off its header
    /// fields, a commented `Message-ID`, a folded `Subject` and a word in a
    /// charset that decodes nothing among them, never off an `ENVELOPE`.
    #[test]
    fn a_message_fetched_alone_is_keyed_as_the_vectors_state() {
        let server = FakeImap::start(
            FakeMailbox {
                uid_validity: 1,
                highest_mod_seq: 1,
                messages: (1..=VECTORS.len() as u32)
                    .map(|uid| FakeMessage {
                        uid,
                        flags: Vec::new(),
                        message_id: String::new(),
                    })
                    .collect(),
                states_exists: true,
            },
            false,
        );
        for (uid, vector) in (1..).zip(&VECTORS) {
            server.set_header(uid, vector.header());
        }
        let mut client = ImapClient::connect(&server.url, &Tls::default(), false, None).unwrap();
        let uids: Vec<String> = (1..=VECTORS.len()).map(|uid| uid.to_string()).collect();
        let uids: Vec<&str> = uids.iter().map(String::as_str).collect();

        let named = client.fetch_summaries("INBOX", &uids).unwrap();

        assert_eq!(named.len(), VECTORS.len());
        for (uid, derivation) in &named {
            VECTORS[uid.parse::<usize>().unwrap() - 1].assert_named(derivation);
        }
        assert!(
            server
                .sent("UID FETCH")
                .iter()
                .all(|line| !line.contains("ENVELOPE")),
            "{:?}",
            server.sent("UID FETCH")
        );
    }

    /// A row the server answered without a UID names nothing.
    #[test]
    fn a_row_without_a_uid_is_skipped() {
        let mut row = meta_row(1, 1, "Subject: x\r\n\r\n");
        row.remove(0);
        assert!(meta_from(row).is_none());
    }

    /// The resume cursor carries the `UIDVALIDITY`, the lowest UID listed
    /// and the highest UID the round found; one written before the last
    /// reads it as 0, and anything else is no cursor at all.
    #[test]
    fn a_round_cursor_round_trips_and_rejects_garbage() {
        let cursor = RoundCursor {
            uid_validity: 1_774_329_954,
            lowest: 501,
            top: 1_100,
        };
        assert_eq!(decode_cursor(&encode_cursor(cursor)), Some(cursor));

        let mut legacy = Vec::new();
        legacy.extend_from_slice(&1_774_329_954u32.to_le_bytes());
        legacy.extend_from_slice(&501u32.to_le_bytes());
        assert_eq!(
            decode_cursor(&legacy),
            Some(RoundCursor { top: 0, ..cursor }),
            "a cursor from before lists the whole scope again on its last page"
        );

        assert_eq!(decode_cursor(&[]), None);
        assert_eq!(decode_cursor(&[0; 5]), None);
        assert_eq!(decode_cursor(&[0; 16]), None);
    }

    /// A delta lists from a checkpoint only where QRESYNC can: the server
    /// offers it, and the checkpoint names a `UIDVALIDITY` and a modseq.
    #[test]
    fn a_qresync_position_needs_the_server_and_a_modseq() {
        let checkpoint = encode_checkpoint(7, 42);
        assert_eq!(
            qresync_position(&checkpoint, true),
            Some((NonZeroU32::new(7).unwrap(), 42))
        );
        assert_eq!(qresync_position(&checkpoint, false), None, "no QRESYNC");
        assert_eq!(
            qresync_position(&encode_checkpoint(7, 0), true),
            None,
            "a mailbox without CONDSTORE"
        );
        assert_eq!(qresync_position(&encode_checkpoint(0, 42), true), None);
        assert_eq!(qresync_position(&[1, 2, 3], true), None);
    }

    /// A `VANISHED (EARLIER)` set spanning every UID names only what the
    /// store binds, each handle once however the ranges overlap.
    #[test]
    fn vanished_ranges_name_the_held_handles_only() {
        let handles = HashSet::from(["3", "9", "12", "x"].map(str::to_owned));
        let held = Held {
            handles: &handles,
            checkpoint: None,
            coverage: None,
        };
        let range = |start, end| NonZeroU32::new(start).unwrap()..=NonZeroU32::new(end).unwrap();

        assert_eq!(
            held_vanished(&[range(1, u32::MAX), range(2, 9)], &held),
            ["12", "3", "9"]
        );
        assert_eq!(held_vanished(&[range(4, 8)], &held), Vec::<String>::new());
    }

    /// The store's handles are trusted only under the `UIDVALIDITY` its
    /// checkpoint names: a round or a head under a newer one reads every
    /// message's meta anew.
    #[test]
    fn handles_are_trusted_under_the_store_s_uidvalidity_only() {
        let handles = std::collections::HashSet::from(["3".to_owned()]);
        let checkpoint = encode_checkpoint(7, 42);
        let held = |checkpoint| Held {
            handles: &handles,
            checkpoint,
            coverage: None,
        };

        assert!(trusts(&held(Some(&checkpoint)), 7));
        assert!(!trusts(&held(Some(&checkpoint)), 8));
        assert!(!trusts(&held(None), 7), "a first round binds nothing yet");
    }

    /// A band head reaches two days below the round's opening, by the
    /// server's own days.
    #[test]
    fn a_band_head_reaches_two_days_below_the_round_s_opening() {
        let day = imap_day("2026-10-13T09:30:00Z", -2).unwrap();
        assert_eq!(day.as_ref().to_string(), "2026-10-11");
    }

    /// `SENTSINCE` and `SENTBEFORE` compare dates in the server's own zone,
    /// so the search reaches a day below the floor and two above the
    /// ceiling, the `Date` deciding afterwards.
    #[test]
    fn a_scope_narrows_the_search_with_a_day_of_margin() {
        let since = imap_day("2026-10-01T00:00:00Z", -1).unwrap();
        assert_eq!(since.as_ref().to_string(), "2026-09-30");
        let before = imap_day("2026-10-01T23:30:00+02:00", 2).unwrap();
        assert_eq!(before.as_ref().to_string(), "2026-10-03");
        assert!(imap_day("last week", -1).is_none());
    }

    /// A flag push names only what it changes (pimdir SYNC §4): a flag
    /// another client set after the listing survives a push removing
    /// another one.
    #[test]
    fn a_flag_the_server_added_after_the_listing_survives_a_push() {
        let server = FakeImap::start(
            FakeMailbox {
                uid_validity: 1,
                highest_mod_seq: 1,
                messages: vec![FakeMessage {
                    uid: 1,
                    flags: vec![String::from("\\Seen"), String::from("\\Flagged")],
                    message_id: String::from("a@x"),
                }],
                states_exists: true,
            },
            false,
        );
        let mut client = ImapClient::connect(&server.url, &Tls::default(), false, None).unwrap();

        // NOTE: listed as seen and flagged; another client answers it.
        server.set_flags(1, &["\\Seen", "\\Flagged", "\\Answered"]);
        let flagged = Flag::from_iana(IanaFlag::Flagged);
        client
            .update_flags("INBOX", &["1"], &[], &[flagged])
            .unwrap();

        assert_eq!(server.flags(1), ["\\Seen", "\\Answered"]);
    }

    /// Listed rows keep their flags, and each takes the meta read for it.
    #[test]
    fn listed_rows_take_their_meta_by_uid() {
        let (uid, meta, _) = meta_from(meta_row(9, 1, "Message-ID: <c@x>\r\n\r\n")).unwrap();
        let rows = vec![
            Spine {
                uid: 8,
                flags: BTreeSet::new(),
            },
            Spine {
                uid,
                flags: meta.flags.clone(),
            },
        ];
        let entries = entries(rows, BTreeMap::from([(uid, meta)]));

        assert_eq!(entries[0].id, "8");
        assert!(entries[0].meta.is_none(), "a bound message reads no meta");
        assert_eq!(entries[1].id, "9");
        assert_eq!(
            entries[1].meta.as_ref().map(|meta| meta.link_id.as_str()),
            Some("c@x")
        );
    }
}
