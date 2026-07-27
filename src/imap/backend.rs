//! IMAP adapter for the shared cross-protocol client.
//!
//! Thin glue over [`ImapClient`], which already wraps io_imap's
//! high-level session (`select`, `fetch`, `store`, `copy`, `move`,
//! `append`, `list`, `status`). Each method takes and returns the TUI's
//! shared [`crate::item`] types; the only real work is converting
//! between those and io_imap's wire types, adapted from the retired
//! io-email IMAP drivers.

use std::{
    collections::BTreeSet,
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
        envelope::Address as ImapAddress,
        fetch::{MacroOrMessageDataItemNames, MessageDataItem, MessageDataItemName},
        flag::{Flag as ImapFlag, FlagFetch, FlagNameAttribute, StoreType},
        mailbox::{ListMailbox, Mailbox as ImapMailbox},
        search::SearchKey,
        sequence::SequenceSet,
        status::{StatusDataItem, StatusDataItemName},
    },
};
use rfc2047_decoder::{Decoder, RecoverStrategy};

use crate::{
    client::{EnumEntry, Enumeration},
    imap::client::ImapClient,
    item::{
        address::Address,
        collection::Collection,
        flag::{Flag, FlagOp, IanaFlag},
        summary::{ItemSummary, normalize_message_id, parse_message_ids},
    },
};

impl ImapClient {
    /// Lists every selectable mailbox. With `with_counts`, follows each
    /// row with a STATUS to populate totals and unread counts.
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

    /// Enumerates a mailbox's UID+flag spine, incrementally when possible.
    ///
    /// The opaque `cursor` bytes carry the last `(UIDVALIDITY,
    /// HIGHESTMODSEQ)` pair ([`decode_checkpoint`]); with one and a
    /// QRESYNC-capable server whose UIDVALIDITY still matches, does a QRESYNC
    /// SELECT: the server streams only the messages changed since the modseq and
    /// the UIDs that vanished, so nothing is fetched when nothing changed.
    /// Otherwise a full `FETCH 1:* (UID FLAGS)` snapshot. No ENVELOPE is fetched
    /// — the link id is resolved later at the `Meta` tier.
    pub fn enumerate(&mut self, mailbox: &str, cursor: Option<&[u8]>) -> Result<Enumeration> {
        let mbox = parse_mailbox(mailbox)?;
        let cursor = cursor.and_then(decode_checkpoint);

        // Delta path: QRESYNC SELECT when the cursor matches.
        if let Some((cv, cmodseq)) = cursor
            && cmodseq > 0
            && self.supports_qresync()
            && let Some(cv_nz) = NonZeroU32::new(cv)
        {
            let data = self.select_delta(mbox.clone(), cv_nz, cmodseq)?;
            self.mark_selected(mailbox);
            let uid_validity = data.uid_validity.map(|v| v.get()).unwrap_or(cv);
            let highest_mod_seq = data.highest_mod_seq.unwrap_or(cmodseq);
            // UIDVALIDITY unchanged → the delta is valid; else the mailbox was
            // recreated, so fall through to a full snapshot.
            if uid_validity == cv {
                let items = data
                    .changed
                    .iter()
                    .filter_map(|fetch| enum_entry(&fetch.items.clone().into_inner()))
                    .collect();
                let vanished = data
                    .vanished_earlier
                    .iter()
                    .map(|u| u.get().to_string())
                    .collect();
                return Ok(Enumeration {
                    items,
                    vanished,
                    complete: false,
                    checkpoint: encode_checkpoint(uid_validity, highest_mod_seq),
                });
            }
        }

        // Full path: plain SELECT + FETCH 1:* (UID FLAGS).
        let select = self.select(mbox, ImapMailboxSelectOptions::default())?;
        self.mark_selected(mailbox);
        let uid_validity = select.uid_validity.map(|v| v.get()).unwrap_or(0);
        let highest_mod_seq = select.highest_mod_seq.unwrap_or(0);
        let exists = select.exists.unwrap_or(0);
        let items = if exists == 0 {
            Vec::new()
        } else {
            let sequence_set: SequenceSet = "1:*"
                .try_into()
                .map_err(|_| anyhow!("Invalid IMAP sequence-set `1:*`"))?;
            let data = self.fetch(
                sequence_set,
                uid_flag_names(),
                ImapMessageFetchOptions::default(),
            )?;
            data.into_values()
                .filter_map(|items| enum_entry(&items.into_inner()))
                .collect()
        };
        Ok(Enumeration {
            items,
            vanished: Vec::new(),
            complete: true,
            checkpoint: encode_checkpoint(uid_validity, highest_mod_seq),
        })
    }

    /// SELECTs `mailbox` unless it is already the connection's current selection,
    /// so a run of fetches on one mailbox pays a single SELECT. The select
    /// response is discarded (callers that need `UIDVALIDITY`/`EXISTS` use the
    /// explicit path in `enumerate`).
    fn select_cached(&mut self, mailbox: &str) -> Result<()> {
        if self.is_selected(mailbox) {
            return Ok(());
        }
        let mbox = parse_mailbox(mailbox)?;
        self.select(mbox, ImapMailboxSelectOptions::default())?;
        self.mark_selected(mailbox);
        Ok(())
    }

    /// Fetches envelopes for a specific UID set (link-id / summary resolution at
    /// the `Meta` tier), as `UID FETCH <set> (UID FLAGS ENVELOPE RFC822.SIZE)` —
    /// targeted, never a whole-mailbox `1:*` sweep. Empty `uids` short-circuits.
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

    /// Streams the bodies of a UID set in one batched `UID FETCH … (UID
    /// BODY.PEEK[])`, routing each to a per-message sink (`open` at its start,
    /// `done` at its end). One SELECT + one FETCH for the whole set; no body lands
    /// in memory whole. The inner method is `fetch_bodies_stream` (reached through
    /// `Deref`), so this wrapper is named `fetch_bodies` to avoid recursing.
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
        .with_context(|| format!("Batched body fetch `{mailbox}` error"))?;
        Ok(())
    }

    /// Streams one message's raw RFC 5322 bytes into `sink` without flipping
    /// `\Seen` (BODY.PEEK[]); the body never lands in memory whole.
    pub fn get_message_stream(&mut self, mailbox: &str, id: &str, sink: impl Write) -> Result<()> {
        let uid: NonZeroU32 = id.parse().map_err(|_| anyhow!("Invalid IMAP UID `{id}`"))?;

        self.select_cached(mailbox)?;
        self.fetch_body_stream(uid, true, sink)?;
        Ok(())
    }

    /// Appends `len` octets streamed from `source` to `mailbox` with `flags`,
    /// returning the appended UID (UIDPLUS APPENDUID, else a UID SEARCH on the
    /// provided `message_id`); the body never lands in memory whole.
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

        // No UIDPLUS: recover the UID via SELECT + UID SEARCH on the message's
        // own Message-ID (carried from the link id, since the body streamed
        // past without being parsed).
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

    /// Copies a UID set from `from` to `to`. Part of the backend surface;
    /// the sync engine moves rather than copies.
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

    /// Deletes one message: marks it `\Deleted`, then EXPUNGEs. EXPUNGE
    /// removes every `\Deleted` message in the mailbox, but the sync
    /// engine only flags the ones it means to drop, so nothing else is
    /// caught.
    pub fn delete_message(&mut self, mailbox: &str, id: &str) -> Result<()> {
        let sequence_set = parse_uids(&[id])?;

        self.select_cached(mailbox)?;
        self.store(
            sequence_set,
            StoreType::Add,
            vec![ImapFlag::Deleted],
            ImapMessageStoreOptions { uid: true },
        )?;
        self.expunge()?;

        Ok(())
    }
}

/// One IMAP LIST row (mailbox, delimiter, attributes).
type ListRow = (
    ImapMailbox<'static>,
    Option<QuotedChar>,
    Vec<FlagNameAttribute<'static>>,
);

/// Drops `\Noselect` containers (RFC 3501 §6.3.8): they cannot hold
/// messages and would error out on any later shared-API op.
fn is_selectable(row: &ListRow) -> bool {
    !row.2.contains(&FlagNameAttribute::Noselect)
}

/// Converts one IMAP LIST row into the shared [`Collection`] shape.
fn mailbox_from(row: ListRow) -> Collection {
    let name = match row.0 {
        // NOTE: the RFC 3501 canonical spelling (uppercase). Sync pairs
        // mailboxes by name across backends, so the IMAP INBOX must match
        // the conventional `INBOX`.
        ImapMailbox::Inbox => "INBOX".to_string(),
        ImapMailbox::Other(other) => String::from_utf8_lossy(other.inner().as_ref()).into_owned(),
    };

    Collection {
        id: name.clone(),
        name,
        total: None,
        unread: None,
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

/// FETCH item-name list: UID + FLAGS + ENVELOPE + RFC822.SIZE, plus
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

/// The lean FETCH item set for enumeration: UID + FLAGS only (no ENVELOPE).
fn uid_flag_names() -> MacroOrMessageDataItemNames<'static> {
    MacroOrMessageDataItemNames::MessageDataItemNames(vec![
        MessageDataItemName::Uid,
        MessageDataItemName::Flags,
    ])
}

/// Extracts one enumeration entry (UID + flags) from a FETCH row; `None` when no
/// UID is present.
fn enum_entry(items: &[MessageDataItem<'static>]) -> Option<EnumEntry> {
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
    Some(EnumEntry {
        // IMAP message bodies are immutable, so there is no content revision.
        revision: None,
        id: uid?.to_string(),
        flags,
    })
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
                // NOTE: the 9th ENVELOPE element (RFC 3501 §7.4.2), so the
                // reply's parent costs nothing beyond the FETCH the
                // enumeration already issues.
                if let Some(m) = env.in_reply_to.into_option() {
                    in_reply_to = parse_message_ids(&bytes_to_string(m.as_ref()));
                }
                from = env.from.iter().map(address_from).collect();
                to = env.to.iter().map(address_from).collect();
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

/// Maps a shared [`Flag`] to its IMAP wire counterpart. IANA flags
/// become the matching system flag; custom keywords pass through as
/// Keyword atoms, with a sanitised fallback for non-atom-safe input.
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
        .map_err(|_| anyhow!("Invalid IMAP mailbox `{name}`"))
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
                .map_err(|_| anyhow!("Invalid message UID `{s}`"))
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
/// bytes (little-endian: 4-byte uidvalidity + 8-byte modseq). The encoding is
/// private to this adapter: the driver never decodes it, reading the epoch
/// through `Client::handle_space_epoch` instead.
pub(crate) fn encode_checkpoint(uid_validity: u32, highest_mod_seq: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12);
    bytes.extend_from_slice(&uid_validity.to_le_bytes());
    bytes.extend_from_slice(&highest_mod_seq.to_le_bytes());
    bytes
}

/// Decodes an IMAP sync cursor; `None` for an absent or malformed checkpoint (a
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
/// are not an IMAP cursor. Read through `Client::handle_space_epoch`, which the
/// driver compares before and after a pull to detect a handle-space change
/// (which rebuilds the collection and bumps its generation).
pub(crate) fn checkpoint_uid_validity(bytes: &[u8]) -> Option<u32> {
    decode_checkpoint(bytes).map(|(uid_validity, _)| uid_validity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_round_trips_and_rejects_garbage() {
        // The IMAP cursor survives encode → store → decode.
        let bytes = encode_checkpoint(1_774_329_954, 5035);
        assert_eq!(decode_checkpoint(&bytes), Some((1_774_329_954, 5035)));
        assert_eq!(checkpoint_uid_validity(&bytes), Some(1_774_329_954));
        // A wrong length (an old/foreign checkpoint) decodes to None, which forces
        // a full enumerate rather than a bogus delta.
        assert_eq!(decode_checkpoint(&[]), None);
        assert_eq!(decode_checkpoint(&[0; 8]), None);
        assert_eq!(checkpoint_uid_validity(&[0; 3]), None);
        // A non-CONDSTORE checkpoint (modseq 0) round-trips but the delta guard
        // (`modseq > 0`) makes the next enumerate full.
        assert_eq!(decode_checkpoint(&encode_checkpoint(42, 0)), Some((42, 0)));
    }
}
