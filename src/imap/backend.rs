//! # IMAP adapter
//!
//! Thin glue over [`ImapClient`], which already wraps io_imap's high-level
//! session. The only real work is converting between the shared
//! [`crate::item`] types and io_imap's wire types.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    num::NonZeroU32,
    str::from_utf8,
};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, FixedOffset};
use io_imap::{
    client::ImapClient as _,
    rfc3501::{
        append::ImapMessageAppendOptions, copy::ImapMessageCopyOptions,
        fetch::ImapMessageFetchOptions, search::ImapMessageSearchOptions,
        select::ImapMailboxSelectOptions, store::ImapMessageStoreOptions,
    },
    rfc6851::r#move::ImapMessageMoveOptions,
    types::{
        body::BodyStructure,
        core::{AString, Atom, QuotedChar, Vec1},
        datetime::NaiveDate as ImapDate,
        envelope::Address as ImapAddress,
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
    remote::{PimdirEnumerate, PimdirListing},
    summary::{PimdirDerivation, mail},
};
use log::debug;
use rfc2047_decoder::{Decoder, RecoverStrategy};

use crate::{
    client::{EnumEntry, Enumeration, Held, Listed},
    imap::client::{ImapClient, RoundUids},
    item::{
        address::Address,
        collection::Collection,
        flag::{Flag, FlagOp, IanaFlag},
        summary::{ItemSummary, normalize_message_id, parse_message_ids},
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

        let rows = self.list(reference, pattern)?;

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
                apply_status(mailbox, items);
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

        Ok(self
            .list(reference, pattern)?
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
    /// QRESYNC `SELECT`; a server without QRESYNC, or a checkpoint whose
    /// `UIDVALIDITY` moved, answers the first page of a round instead. A
    /// round lists the scope newest first, [`PAGE_SIZE`] UIDs a page, its
    /// cursor the lowest UID listed; its first page carries the checkpoint
    /// taken at the `SELECT`, so what moves during the round is the next
    /// delta's.
    pub fn enumerate(
        &mut self,
        mailbox: &str,
        request: &PimdirEnumerate,
        held: Held<'_>,
    ) -> Result<Listed> {
        match &request.listing {
            PimdirListing::Delta(checkpoint) => {
                self.delta_page(mailbox, &checkpoint.0, &request.scope, held)
            }
            PimdirListing::Round { cursor, band } => self.round_page(
                mailbox,
                cursor.as_ref().map(|cursor| cursor.0.as_slice()),
                *band,
                &request.scope,
                held,
            ),
        }
    }

    /// What changed since `checkpoint`, or a round's first page when the
    /// server cannot say.
    fn delta_page(
        &mut self,
        mailbox: &str,
        checkpoint: &[u8],
        scope: &PimdirScope,
        held: Held<'_>,
    ) -> Result<Listed> {
        let Some((cv, cmodseq)) = decode_checkpoint(checkpoint) else {
            return self.round_page(mailbox, None, false, scope, held);
        };
        let Some(cv_nz) = NonZeroU32::new(cv).filter(|_| cmodseq > 0 && self.supports_qresync())
        else {
            return self.round_page(mailbox, None, false, scope, held);
        };

        let mbox = parse_mailbox(mailbox)?;
        let data = self.select_delta(mbox, cv_nz, cmodseq)?;
        self.mark_selected(mailbox);
        let uid_validity = data.uid_validity.map(|v| v.get()).unwrap_or(cv);
        if uid_validity != cv {
            debug!("UIDVALIDITY of {mailbox} moved, listing it in a round");
            return self.round_page(mailbox, None, false, scope, held);
        }
        let highest_mod_seq = data.highest_mod_seq.unwrap_or(cmodseq);

        let changed: Vec<Spine> = data
            .changed
            .iter()
            .filter_map(|fetch| spine(&fetch.items.clone().into_inner()))
            .collect();
        let vanished = data
            .vanished_earlier
            .iter()
            .map(|uid| uid.get().to_string())
            .collect();

        // NOTE: the handles still name what the store binds, the
        // `UIDVALIDITY` being the checkpoint's.
        let unheld: Vec<u32> = changed
            .iter()
            .filter(|row| !held.holds(&row.uid.to_string()))
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
        Ok(Listed::Page(page))
    }

    /// One page of a round over `scope`, from its start or from `cursor`.
    fn round_page(
        &mut self,
        mailbox: &str,
        cursor: Option<&[u8]>,
        band: bool,
        scope: &PimdirScope,
        held: Held<'_>,
    ) -> Result<Listed> {
        let cursor = match cursor {
            None => None,
            Some(bytes) => match decode_cursor(bytes) {
                Some(cursor) => Some(cursor),
                None => return Ok(Listed::CursorRejected),
            },
        };

        let mbox = parse_mailbox(mailbox)?;
        let select = self.select(mbox, ImapMailboxSelectOptions::default())?;
        self.mark_selected(mailbox);
        let uid_validity = select.uid_validity.map(|v| v.get()).unwrap_or(0);
        let highest_mod_seq = select.highest_mod_seq.unwrap_or(0);

        if let Some((validity, _)) = cursor
            && validity != uid_validity
        {
            debug!("UIDVALIDITY of {mailbox} moved under the round, restarting it");
            return Ok(Listed::CursorRejected);
        }

        let key = (scope.since.clone(), scope.until.clone());
        let below = cursor.map(|(_, lowest)| lowest);
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
            None => self.search_round(scope, below, select.exists.unwrap_or(0))?,
        };

        let rest = left.split_off(left.len().min(PAGE_SIZE));
        let page_uids = left;
        let next = rest
            .first()
            .and(page_uids.last())
            .map(|lowest| encode_cursor(uid_validity, *lowest));
        if !rest.is_empty() {
            self.round = Some(RoundUids {
                mailbox: mailbox.to_owned(),
                uid_validity,
                scope: key,
                left: rest,
            });
        }

        // NOTE: a handle names what the store binds only under the
        // `UIDVALIDITY` its checkpoint was made under.
        let trusted = held
            .checkpoint
            .and_then(checkpoint_uid_validity)
            .is_some_and(|validity| validity == uid_validity);
        let (heldset, unheld): (Vec<u32>, Vec<u32>) = page_uids
            .iter()
            .partition(|uid| trusted && held.holds(&uid.to_string()));

        let mut rows = self.fetch_spines(&heldset)?;
        let (metas, bytes) = self.fetch_metas(&unheld)?;
        rows.extend(metas.iter().map(|(uid, meta)| Spine {
            uid: *uid,
            flags: meta.flags.clone(),
        }));
        let items = entries(rows, metas);

        let first = cursor.is_none();
        Ok(Listed::Page(Enumeration {
            items,
            vanished: Vec::new(),
            complete: true,
            cursor: next,
            checkpoint: (first && !band).then(|| encode_checkpoint(uid_validity, highest_mod_seq)),
            bytes,
        }))
    }

    /// The UIDs of a round, highest first: every message, or the ones
    /// `SENTSINCE` and `SENTBEFORE` keep with a day's margin around the
    /// scope (the server compares dates, not instants, in its own zone) and
    /// the ones with no `Date`, below `below` when a round resumes.
    fn search_round(
        &mut self,
        scope: &PimdirScope,
        below: Option<u32>,
        exists: u32,
    ) -> Result<Vec<u32>> {
        if exists == 0 {
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

        let mut uids: Vec<u32> = self
            .search(criteria, ImapMessageSearchOptions { uid: true })?
            .into_iter()
            .map(NonZeroU32::get)
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
            let data = self.fetch(
                set,
                uid_flag_names(),
                ImapMessageFetchOptions {
                    uid: true,
                    ..Default::default()
                },
            )?;
            rows.extend(
                data.into_values()
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
            let data = self.fetch(
                set,
                meta_names()?,
                ImapMessageFetchOptions {
                    uid: true,
                    ..Default::default()
                },
            )?;
            for items in data.into_values() {
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
        self.select(mbox, ImapMailboxSelectOptions::default())?;
        self.mark_selected(mailbox);
        Ok(())
    }

    /// Fetches envelopes for a specific UID set, as `UID FETCH <set> (UID FLAGS
    /// ENVELOPE RFC822.SIZE)`: targeted, never a whole-mailbox `1:*` sweep.
    pub fn fetch_envelopes(&mut self, mailbox: &str, uids: &[&str]) -> Result<Vec<ItemSummary>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let sequence_set = parse_uids(uids)?;

        self.select_cached(mailbox)?;
        let data = self.fetch(
            sequence_set,
            build_item_names(false),
            ImapMessageFetchOptions {
                uid: true,
                ..Default::default()
            },
        )?;

        Ok(data
            .into_iter()
            .map(|(seq, items)| envelope_from(seq.get(), items.into_inner()))
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

    /// Streams a UID set's bodies in one batched `UID FETCH … (BODY.PEEK[])`.
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
    ) -> Result<()> {
        let sequence_set = parse_uids(ids)?;
        self.select_cached(mailbox)?;
        self.fetch_bodies_stream(
            sequence_set,
            true,
            |uid| open(&uid.to_string()),
            |uid, sink| done(&uid.to_string(), None, sink),
        )
        .with_context(|| format!("Batched body fetch {mailbox} error"))?;
        Ok(())
    }

    /// Streams one message's raw RFC 5322 bytes into `sink` without flipping
    /// `\Seen` (BODY.PEEK[]).
    pub fn get_message_stream(&mut self, mailbox: &str, id: &str, sink: impl Write) -> Result<()> {
        let uid: NonZeroU32 = id.parse().map_err(|_| anyhow!("Invalid IMAP UID {id}"))?;

        self.select_cached(mailbox)?;
        self.fetch_body_stream(uid, true, sink)?;
        Ok(())
    }

    /// Appends `len` octets streamed from `source` to `mailbox` with `flags`.
    ///
    /// Returns the appended UID from the UIDPLUS APPENDUID response, falling
    /// back to a UID SEARCH on `message_id` where the server lacks UIDPLUS.
    pub fn add_message_stream(
        &mut self,
        mailbox: &str,
        flags: &[Flag],
        source: impl Read,
        len: usize,
        message_id: Option<&str>,
    ) -> Result<String> {
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
            return Ok(uid.to_string());
        }

        let message_id = message_id.map(str::trim).filter(|id| !id.is_empty());
        let Some(message_id) = message_id else {
            bail!(
                "Cannot resolve appended UID: server lacks UIDPLUS and no Message-ID was provided"
            );
        };

        self.select_cached(mailbox)?;

        let field =
            AString::try_from("Message-ID").map_err(|_| anyhow!("Invalid IMAP search header"))?;
        let value = AString::try_from(message_id.to_string())
            .map_err(|_| anyhow!("Invalid IMAP search Message-ID value"))?;
        let criteria = Vec1::from(SearchKey::Header(field, value));
        let uids = self.search(criteria, ImapMessageSearchOptions { uid: true })?;

        uids.into_iter()
            .max()
            .map(|uid| uid.to_string())
            .ok_or_else(|| anyhow!("Fallback UID search returned no match"))
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

        let rows = self.list(reference, pattern)?;
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

/// FETCH item-name list: UID, FLAGS, ENVELOPE and RFC822.SIZE, plus
/// BODYSTRUCTURE when `with_attachment` is set.
fn build_item_names(with_attachment: bool) -> MacroOrMessageDataItemNames<'static> {
    let mut names = vec![
        MessageDataItemName::Uid,
        MessageDataItemName::Flags,
        MessageDataItemName::Envelope,
        MessageDataItemName::Rfc822Size,
    ];
    if with_attachment {
        names.push(MessageDataItemName::BodyStructure);
    }
    MacroOrMessageDataItemNames::MessageDataItemNames(names)
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

/// Encodes a round's resume cursor `(UIDVALIDITY, lowest UID listed)`:
/// little-endian, 4 bytes each.
fn encode_cursor(uid_validity: u32, lowest: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&uid_validity.to_le_bytes());
    bytes.extend_from_slice(&lowest.to_le_bytes());
    bytes
}

/// Decodes a round's resume cursor; `None` for bytes that are not one.
fn decode_cursor(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() != 8 {
        return None;
    }
    let uid_validity = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
    let lowest = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    Some((uid_validity, lowest))
}

/// Folds one FETCH row into a shared [`ItemSummary`].
fn envelope_from(seq: u32, items: Vec<MessageDataItem<'static>>) -> ItemSummary {
    let mut id = String::new();
    let mut message_id: Option<String> = None;
    let mut in_reply_to = Vec::new();
    let mut flags = BTreeSet::new();
    let mut subject = String::new();
    let mut from = Vec::new();
    let mut to = Vec::new();
    let mut cc = Vec::new();
    let mut bcc = Vec::new();
    let mut date: Option<DateTime<FixedOffset>> = None;
    let mut size: u64 = 0;
    let mut has_attachment: Option<bool> = None;

    for item in items {
        match item {
            MessageDataItem::Uid(uid) => id = uid.get().to_string(),
            MessageDataItem::Flags(fs) => {
                flags = fs.into_iter().filter_map(flag_from_fetch).collect();
            }
            MessageDataItem::Envelope(env) => {
                if let Some(s) = env.subject.into_option() {
                    subject = decode_mime_bytes(s.as_ref());
                }
                if let Some(d) = env.date.into_option() {
                    date = parse_rfc2822_date(&bytes_to_string(d.as_ref()));
                }
                if let Some(m) = env.message_id.into_option() {
                    message_id = normalize_message_id(&bytes_to_string(m.as_ref()));
                }
                if let Some(m) = env.in_reply_to.into_option() {
                    in_reply_to = parse_message_ids(&bytes_to_string(m.as_ref()));
                }
                from = env.from.iter().map(address_from).collect();
                to = env.to.iter().map(address_from).collect();
                cc = env.cc.iter().map(address_from).collect();
                bcc = env.bcc.iter().map(address_from).collect();
            }
            MessageDataItem::Rfc822Size(n) => size = u64::from(n),
            MessageDataItem::BodyStructure(structure) => {
                has_attachment = Some(body_structure_has_attachment(&structure));
            }
            _ => {}
        }
    }

    if id.is_empty() {
        id = seq.to_string();
    }

    ItemSummary {
        id,
        message_id,
        in_reply_to,
        flags,
        subject,
        from,
        to,
        cc,
        bcc,
        date,
        size,
        has_attachment,
    }
}

fn flag_from_fetch(fetch: FlagFetch<'_>) -> Option<Flag> {
    let FlagFetch::Flag(flag) = fetch else {
        return None;
    };
    Some(Flag::from_raw(flag.to_string()))
}

fn address_from(addr: &ImapAddress<'_>) -> Address {
    let name = addr
        .name
        .0
        .as_ref()
        .map(|s| decode_mime_bytes(s.as_ref()))
        .filter(|s| !s.is_empty());

    let mailbox = addr
        .mailbox
        .0
        .as_ref()
        .map(|s| bytes_to_string(s.as_ref()))
        .unwrap_or_default();
    let host = addr
        .host
        .0
        .as_ref()
        .map(|s| bytes_to_string(s.as_ref()))
        .unwrap_or_default();

    let email = if mailbox.is_empty() {
        host
    } else if host.is_empty() {
        mailbox
    } else {
        format!("{mailbox}@{host}")
    };

    Address { name, email }
}

fn body_structure_has_attachment(structure: &BodyStructure<'_>) -> bool {
    match structure {
        BodyStructure::Single { extension_data, .. } => extension_data
            .as_ref()
            .and_then(|ext| ext.tail.as_ref())
            .and_then(|disposition| disposition.disposition.as_ref())
            .map(|(kind, _)| kind.as_ref().eq_ignore_ascii_case(b"attachment"))
            .unwrap_or(false),
        BodyStructure::Multi { bodies, .. } => {
            bodies.as_ref().iter().any(body_structure_has_attachment)
        }
    }
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

fn parse_rfc2822_date(raw: &str) -> Option<DateTime<FixedOffset>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc2822(trimmed).ok()
}

fn bytes_to_string(bytes: &[u8]) -> String {
    from_utf8(bytes).map(str::to_string).unwrap_or_else(|_| {
        let mut out = String::with_capacity(bytes.len());
        for b in bytes {
            out.push(*b as char);
        }
        out
    })
}

/// Decodes RFC 2047 MIME-encoded words from IMAP ENVELOPE strings;
/// falls back to [`bytes_to_string`] on malformed input.
fn decode_mime_bytes(bytes: &[u8]) -> String {
    let decoder = Decoder::new().too_long_encoded_word_strategy(RecoverStrategy::Decode);
    decoder
        .decode(bytes)
        .unwrap_or_else(|_| bytes_to_string(bytes))
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
    use super::*;

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

    /// A row the server answered without a UID names nothing.
    #[test]
    fn a_row_without_a_uid_is_skipped() {
        let mut row = meta_row(1, 1, "Subject: x\r\n\r\n");
        row.remove(0);
        assert!(meta_from(row).is_none());
    }

    /// The resume cursor carries the `UIDVALIDITY` and the lowest UID
    /// listed, and anything else is no cursor at all.
    #[test]
    fn a_round_cursor_round_trips_and_rejects_garbage() {
        let cursor = encode_cursor(1_774_329_954, 501);
        assert_eq!(decode_cursor(&cursor), Some((1_774_329_954, 501)));
        assert_eq!(decode_cursor(&[]), None);
        assert_eq!(
            decode_cursor(&encode_checkpoint(1, 2)),
            None,
            "a checkpoint is no cursor"
        );
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
