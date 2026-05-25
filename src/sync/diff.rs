// This file is part of Neverest, a CLI to synchronize emails.
//
// Copyright (C) 2024-2026  soywod <pimalaya.org@posteo.net>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Pure diff math: takes snapshots and live listings, emits hunks
//! pre-gated by [`SidePermissions`].

use std::{
    collections::{BTreeSet, HashMap, HashSet, hash_map::DefaultHasher, hash_map::Entry},
    hash::{Hash, Hasher},
};

use io_email::{
    envelope::{Envelope, FlagUpdate},
    flag::Flag,
};

use crate::{
    config::{MailboxFilter, SidePermissions},
    side::Side,
    sync::{
        cache::{MessageEntry, MessageSnapshots},
        hunk::{EmailHunk, MailboxHunk},
        report::MessageCollision,
    },
};

/// 64-bit cross-side message identifier: hashed `Message-ID:` when
/// present, falling back to `(subject, date, from)`.
pub fn message_key(env: &Envelope) -> u64 {
    let mut hasher = DefaultHasher::new();
    if let Some(message_id) = env.message_id.as_deref() {
        // NOTE: tag so a Message-ID hash cannot collide with a
        // fallback hash for a different message.
        b"mid".hash(&mut hasher);
        message_id.hash(&mut hasher);
        return hasher.finish();
    }
    b"legacy".hash(&mut hasher);
    env.subject.hash(&mut hasher);
    if let Some(date) = env.date {
        date.timestamp().hash(&mut hasher);
    } else {
        0_i64.hash(&mut hasher);
    }
    for addr in &env.from {
        addr.email.hash(&mut hasher);
    }
    hasher.finish()
}

/// Live envelopes keyed by [`message_key`] content hash.
pub type MessageMap<'a> = HashMap<u64, &'a Envelope>;

/// Owned `(content_key, Envelope)` pair list backing a [`MessageMap`].
pub type EnvelopePairs = Vec<(u64, Envelope)>;

/// Re-keys a live envelope listing by content hash.
pub fn pairs_from_envelopes(messages: Vec<Envelope>) -> EnvelopePairs {
    messages.into_iter().map(|m| (message_key(&m), m)).collect()
}

/// Views an [`EnvelopePairs`] as a [`MessageMap`]; on a content-key
/// collision the first envelope wins and duplicates are reported.
pub fn message_map<'a>(
    side: Side,
    mailbox: &str,
    pairs: &'a EnvelopePairs,
    collisions: &mut Vec<MessageCollision>,
) -> MessageMap<'a> {
    let mut out: MessageMap<'a> = HashMap::with_capacity(pairs.len());
    let mut groups: HashMap<u64, usize> = HashMap::new();
    for (key, env) in pairs {
        match out.entry(*key) {
            Entry::Vacant(slot) => {
                slot.insert(env);
            }
            Entry::Occupied(slot) => {
                let prev = *slot.get();
                let index = match groups.entry(*key) {
                    Entry::Vacant(g) => {
                        let new_index = collisions.len();
                        collisions.push(MessageCollision {
                            side,
                            mailbox: mailbox.to_string(),
                            message_id: prev.message_id.clone(),
                            ids: vec![prev.id.clone()],
                        });
                        *g.insert(new_index)
                    }
                    Entry::Occupied(g) => *g.get(),
                };
                collisions[index].ids.push(env.id.clone());
            }
        }
    }
    out
}

/// Re-shapes an [`EnvelopePairs`] into the cache's
/// [`MessageSnapshots`] layout.
pub fn pairs_to_snapshot(pairs: &EnvelopePairs) -> MessageSnapshots {
    pairs
        .iter()
        .map(|(key, envelope)| {
            (
                key.to_string(),
                MessageEntry {
                    id: envelope.id.clone(),
                    flags: envelope.flags.clone(),
                },
            )
        })
        .collect()
}

/// Synthesizes an [`EnvelopePairs`] from a prior snapshot plus the
/// incremental delta (flag updates, new envelopes, vanished ids).
pub fn pairs_from_delta(
    prev: &MessageSnapshots,
    flag_updates: Vec<FlagUpdate>,
    new_envelopes: Vec<Envelope>,
    vanished_ids: HashSet<String>,
) -> EnvelopePairs {
    let updates: HashMap<String, BTreeSet<Flag>> =
        flag_updates.into_iter().map(|u| (u.id, u.flags)).collect();

    let mut out = Vec::with_capacity(prev.len() + new_envelopes.len());

    for (key_str, entry) in prev {
        if vanished_ids.contains(&entry.id) {
            continue;
        }
        let key = key_str.parse::<u64>().unwrap_or_default();
        let flags = updates
            .get(&entry.id)
            .cloned()
            .unwrap_or_else(|| entry.flags.clone());
        out.push((key, stub_envelope(entry.id.clone(), flags)));
    }

    for env in new_envelopes {
        let key = message_key(&env);
        out.push((key, env));
    }

    out
}

/// Envelope shell with only `id` and `flags`; everything the
/// diff/apply paths don't consult is left default.
fn stub_envelope(id: String, flags: BTreeSet<Flag>) -> Envelope {
    Envelope {
        id,
        message_id: None,
        flags,
        subject: String::new(),
        from: Vec::new(),
        to: Vec::new(),
        date: None,
        size: 0,
        has_attachment: None,
    }
}

/// Applies a [`MailboxFilter`] to a freshly listed mailbox-name set.
pub fn filter_mailboxes(all: &HashSet<String>, filter: &MailboxFilter) -> HashSet<String> {
    match filter {
        MailboxFilter::All => all.clone(),
        MailboxFilter::Include(names) => all
            .iter()
            .filter(|m| names.iter().any(|n| n.eq_ignore_ascii_case(m)))
            .cloned()
            .collect(),
        MailboxFilter::Exclude(names) => all
            .iter()
            .filter(|m| !names.iter().any(|n| n.eq_ignore_ascii_case(m)))
            .cloned()
            .collect(),
    }
}

/// Mailbox-level three-way diff: classifies asymmetries via the
/// cached snapshot's last-known mailbox set per side.
pub fn diff_mailboxes(
    left: &HashSet<String>,
    right: &HashSet<String>,
    prev_left: &HashSet<String>,
    prev_right: &HashSet<String>,
    left_perms: SidePermissions,
    right_perms: SidePermissions,
) -> Vec<MailboxHunk> {
    let mut hunks = Vec::new();
    for name in left.difference(right) {
        if prev_right.contains(name) {
            if left_perms.mailbox.delete {
                hunks.push(MailboxHunk::Delete {
                    side: Side::Left,
                    mailbox: name.clone(),
                });
            }
        } else if right_perms.mailbox.create {
            hunks.push(MailboxHunk::Create {
                side: Side::Right,
                mailbox: name.clone(),
            });
        }
    }
    for name in right.difference(left) {
        if prev_left.contains(name) {
            if right_perms.mailbox.delete {
                hunks.push(MailboxHunk::Delete {
                    side: Side::Right,
                    mailbox: name.clone(),
                });
            }
        } else if left_perms.mailbox.create {
            hunks.push(MailboxHunk::Create {
                side: Side::Left,
                mailbox: name.clone(),
            });
        }
    }
    hunks
}

/// Message-level three-way diff for one mailbox; emits
/// `Copy`/`Delete` and delegates flag-only divergences to
/// [`diff_flags`].
pub fn diff_messages(
    mailbox: &str,
    left: &MessageMap<'_>,
    right: &MessageMap<'_>,
    prev_left: &MessageSnapshots,
    prev_right: &MessageSnapshots,
    left_perms: SidePermissions,
    right_perms: SidePermissions,
) -> Vec<EmailHunk> {
    let mut hunks = Vec::new();

    for (key, m) in left {
        let key_str = key.to_string();
        match right.get(key) {
            Some(right_m) => {
                hunks.extend(diff_flags(
                    mailbox,
                    *key,
                    m,
                    right_m,
                    prev_left.get(&key_str),
                    prev_right.get(&key_str),
                    left_perms,
                    right_perms,
                ));
            }
            None => {
                if prev_right.contains_key(&key_str) {
                    if left_perms.message.delete {
                        hunks.push(EmailHunk::Delete {
                            side: Side::Left,
                            mailbox: mailbox.to_string(),
                            id: m.id.clone(),
                            content_key: *key,
                        });
                    }
                } else if right_perms.message.create {
                    hunks.push(EmailHunk::Copy {
                        source_side: Side::Left,
                        target_side: Side::Right,
                        mailbox: mailbox.to_string(),
                        source_id: m.id.clone(),
                        flags: m.flags.clone(),
                        content_key: *key,
                    });
                }
            }
        }
    }

    for (key, m) in right {
        if left.contains_key(key) {
            continue;
        }
        let key_str = key.to_string();
        if prev_left.contains_key(&key_str) {
            if right_perms.message.delete {
                hunks.push(EmailHunk::Delete {
                    side: Side::Right,
                    mailbox: mailbox.to_string(),
                    id: m.id.clone(),
                    content_key: *key,
                });
            }
        } else if left_perms.message.create {
            hunks.push(EmailHunk::Copy {
                source_side: Side::Right,
                target_side: Side::Left,
                mailbox: mailbox.to_string(),
                source_id: m.id.clone(),
                flags: m.flags.clone(),
                content_key: *key,
            });
        }
    }

    hunks
}

/// Flag-level diff for a pair of messages present on both sides;
/// `\Deleted` is treated as a delete-message verb rather than a flag.
pub fn diff_flags(
    mailbox: &str,
    content_key: u64,
    left: &Envelope,
    right: &Envelope,
    prev_left: Option<&MessageEntry>,
    prev_right: Option<&MessageEntry>,
    left_perms: SidePermissions,
    right_perms: SidePermissions,
) -> Vec<EmailHunk> {
    let mut hunks = Vec::new();

    let mut left_deleted_seen = false;
    let mut right_deleted_seen = false;
    let left_flags: BTreeSet<Flag> = left
        .flags
        .iter()
        .filter(|f| {
            if f.is_deleted() {
                left_deleted_seen = true;
                false
            } else {
                true
            }
        })
        .cloned()
        .collect();
    let right_flags: BTreeSet<Flag> = right
        .flags
        .iter()
        .filter(|f| {
            if f.is_deleted() {
                right_deleted_seen = true;
                false
            } else {
                true
            }
        })
        .cloned()
        .collect();

    if left_deleted_seen && !right_deleted_seen && left_perms.message.delete {
        hunks.push(EmailHunk::Delete {
            side: Side::Left,
            mailbox: mailbox.to_string(),
            id: left.id.clone(),
            content_key,
        });
    }
    if right_deleted_seen && !left_deleted_seen && right_perms.message.delete {
        hunks.push(EmailHunk::Delete {
            side: Side::Right,
            mailbox: mailbox.to_string(),
            id: right.id.clone(),
            content_key,
        });
    }

    let prev_left_flags: BTreeSet<Flag> = prev_left
        .map(|e| {
            e.flags
                .iter()
                .filter(|f| !f.is_deleted())
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let prev_right_flags: BTreeSet<Flag> = prev_right
        .map(|e| {
            e.flags
                .iter()
                .filter(|f| !f.is_deleted())
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    let mut to_add_right: BTreeSet<Flag> = BTreeSet::new();
    let mut to_add_left: BTreeSet<Flag> = BTreeSet::new();
    let mut to_remove_right: BTreeSet<Flag> = BTreeSet::new();
    let mut to_remove_left: BTreeSet<Flag> = BTreeSet::new();

    for flag in left_flags.difference(&right_flags) {
        if prev_right_flags.contains(flag) {
            to_remove_left.insert(flag.clone());
        } else {
            to_add_right.insert(flag.clone());
        }
    }
    for flag in right_flags.difference(&left_flags) {
        if prev_left_flags.contains(flag) {
            to_remove_right.insert(flag.clone());
        } else {
            to_add_left.insert(flag.clone());
        }
    }

    if !to_add_right.is_empty() && right_perms.flag.update {
        hunks.push(EmailHunk::AddFlags {
            side: Side::Right,
            mailbox: mailbox.to_string(),
            id: right.id.clone(),
            flags: to_add_right,
            content_key,
        });
    }
    if !to_add_left.is_empty() && left_perms.flag.update {
        hunks.push(EmailHunk::AddFlags {
            side: Side::Left,
            mailbox: mailbox.to_string(),
            id: left.id.clone(),
            flags: to_add_left,
            content_key,
        });
    }
    if !to_remove_right.is_empty() && right_perms.flag.update {
        hunks.push(EmailHunk::RemoveFlags {
            side: Side::Right,
            mailbox: mailbox.to_string(),
            id: right.id.clone(),
            flags: to_remove_right,
            content_key,
        });
    }
    if !to_remove_left.is_empty() && left_perms.flag.update {
        hunks.push(EmailHunk::RemoveFlags {
            side: Side::Left,
            mailbox: mailbox.to_string(),
            id: left.id.clone(),
            flags: to_remove_left,
            content_key,
        });
    }
    hunks
}

#[cfg(test)]
mod tests {
    use io_email::flag::{Flag, IanaFlag};

    use super::*;
    use crate::config::{
        FlagSidePermissions, MailboxSidePermissions, MessageSidePermissions, SidePermissions,
    };

    fn perms_all() -> SidePermissions {
        SidePermissions {
            mailbox: MailboxSidePermissions {
                create: true,
                delete: true,
            },
            flag: FlagSidePermissions { update: true },
            message: MessageSidePermissions {
                create: true,
                delete: true,
            },
        }
    }

    fn perms_with(
        mailbox: MailboxSidePermissions,
        flag: FlagSidePermissions,
        message: MessageSidePermissions,
    ) -> SidePermissions {
        SidePermissions {
            mailbox,
            flag,
            message,
        }
    }

    fn name_set<I: IntoIterator<Item = &'static str>>(names: I) -> HashSet<String> {
        names.into_iter().map(String::from).collect()
    }

    fn envelope(id: &str, message_id: Option<&str>, flags: &[Flag]) -> Envelope {
        Envelope {
            id: id.to_string(),
            message_id: message_id.map(str::to_string),
            flags: flags.iter().cloned().collect(),
            subject: String::new(),
            from: Vec::new(),
            to: Vec::new(),
            date: None,
            size: 0,
            has_attachment: None,
        }
    }

    fn entry(id: &str, flags: &[Flag]) -> MessageEntry {
        MessageEntry {
            id: id.to_string(),
            flags: flags.iter().cloned().collect(),
        }
    }

    #[test]
    fn filter_all_keeps_everything() {
        let all = name_set(["INBOX", "Sent", "Drafts"]);
        assert_eq!(filter_mailboxes(&all, &MailboxFilter::All), all);
    }

    #[test]
    fn filter_include_is_case_insensitive() {
        let all = name_set(["INBOX", "Sent"]);
        let filter = MailboxFilter::Include(vec!["inbox".into()]);
        assert_eq!(filter_mailboxes(&all, &filter), name_set(["INBOX"]));
    }

    #[test]
    fn filter_exclude_drops_named() {
        let all = name_set(["INBOX", "Sent"]);
        let filter = MailboxFilter::Exclude(vec!["sent".into()]);
        assert_eq!(filter_mailboxes(&all, &filter), name_set(["INBOX"]));
    }

    #[test]
    fn diff_mailboxes_no_change_no_hunks() {
        let left = name_set(["INBOX"]);
        let right = name_set(["INBOX"]);
        let prev_left = left.clone();
        let prev_right = right.clone();
        let hunks = diff_mailboxes(
            &left,
            &right,
            &prev_left,
            &prev_right,
            perms_all(),
            perms_all(),
        );
        assert!(hunks.is_empty());
    }

    #[test]
    fn diff_mailboxes_right_deleted_propagates_delete_on_left() {
        let left = name_set(["INBOX"]);
        let right = HashSet::new();
        let prev_left = name_set(["INBOX"]);
        let prev_right = name_set(["INBOX"]);
        let hunks = diff_mailboxes(
            &left,
            &right,
            &prev_left,
            &prev_right,
            perms_all(),
            perms_all(),
        );
        assert_eq!(hunks.len(), 1);
        assert!(matches!(
            &hunks[0],
            MailboxHunk::Delete { side: Side::Left, mailbox } if mailbox == "INBOX"
        ));
    }

    #[test]
    fn diff_mailboxes_left_added_propagates_create_on_right() {
        let left = name_set(["INBOX"]);
        let right = HashSet::new();
        let prev_left = HashSet::new();
        let prev_right = HashSet::new();
        let hunks = diff_mailboxes(
            &left,
            &right,
            &prev_left,
            &prev_right,
            perms_all(),
            perms_all(),
        );
        assert_eq!(hunks.len(), 1);
        assert!(matches!(
            &hunks[0],
            MailboxHunk::Create { side: Side::Right, mailbox } if mailbox == "INBOX"
        ));
    }

    #[test]
    fn diff_mailboxes_right_added_propagates_create_on_left() {
        let left = HashSet::new();
        let right = name_set(["Archive"]);
        let prev_left = HashSet::new();
        let prev_right = HashSet::new();
        let hunks = diff_mailboxes(
            &left,
            &right,
            &prev_left,
            &prev_right,
            perms_all(),
            perms_all(),
        );
        assert_eq!(hunks.len(), 1);
        assert!(matches!(
            &hunks[0],
            MailboxHunk::Create { side: Side::Left, mailbox } if mailbox == "Archive"
        ));
    }

    #[test]
    fn diff_mailboxes_left_deleted_propagates_delete_on_right() {
        let left = HashSet::new();
        let right = name_set(["INBOX"]);
        let prev_left = name_set(["INBOX"]);
        let prev_right = name_set(["INBOX"]);
        let hunks = diff_mailboxes(
            &left,
            &right,
            &prev_left,
            &prev_right,
            perms_all(),
            perms_all(),
        );
        assert_eq!(hunks.len(), 1);
        assert!(matches!(
            &hunks[0],
            MailboxHunk::Delete { side: Side::Right, mailbox } if mailbox == "INBOX"
        ));
    }

    #[test]
    fn diff_mailboxes_blocked_by_permissions() {
        let left = name_set(["INBOX"]);
        let right = HashSet::new();
        let prev_left = name_set(["INBOX"]);
        let prev_right = name_set(["INBOX"]);
        let left_perms = perms_with(
            MailboxSidePermissions {
                create: true,
                delete: false,
            },
            FlagSidePermissions { update: true },
            MessageSidePermissions {
                create: true,
                delete: true,
            },
        );
        let hunks = diff_mailboxes(
            &left,
            &right,
            &prev_left,
            &prev_right,
            left_perms,
            perms_all(),
        );
        assert!(hunks.is_empty());
    }

    #[test]
    fn diff_messages_no_change_no_hunks() {
        let envs = vec![(1u64, envelope("1", Some("<a>"), &[]))];
        let pairs_left = envs.clone();
        let pairs_right = envs;
        let mut collisions = Vec::new();
        let left = message_map(Side::Left, "INBOX", &pairs_left, &mut collisions);
        let right = message_map(Side::Right, "INBOX", &pairs_right, &mut collisions);
        assert!(collisions.is_empty());

        let mut prev_left = MessageSnapshots::new();
        prev_left.insert("1".into(), entry("1", &[]));
        let prev_right = prev_left.clone();

        let hunks = diff_messages(
            "INBOX",
            &left,
            &right,
            &prev_left,
            &prev_right,
            perms_all(),
            perms_all(),
        );
        assert!(hunks.is_empty());
    }

    #[test]
    fn diff_messages_right_vanished_emits_delete_on_left() {
        let left_pairs = vec![(1u64, envelope("L1", Some("<a>"), &[]))];
        let right_pairs: EnvelopePairs = Vec::new();
        let mut collisions = Vec::new();
        let left = message_map(Side::Left, "INBOX", &left_pairs, &mut collisions);
        let right = message_map(Side::Right, "INBOX", &right_pairs, &mut collisions);
        assert!(collisions.is_empty());

        let mut prev_right = MessageSnapshots::new();
        prev_right.insert("1".into(), entry("R1", &[]));

        let hunks = diff_messages(
            "INBOX",
            &left,
            &right,
            &MessageSnapshots::new(),
            &prev_right,
            perms_all(),
            perms_all(),
        );
        assert_eq!(hunks.len(), 1);
        assert!(matches!(
            &hunks[0],
            EmailHunk::Delete {
                side: Side::Left,
                mailbox,
                id,
                ..
            } if mailbox == "INBOX" && id == "L1"
        ));
    }

    #[test]
    fn diff_messages_left_added_emits_copy_to_right() {
        let left_pairs = vec![(1u64, envelope("L1", Some("<a>"), &[]))];
        let right_pairs: EnvelopePairs = Vec::new();
        let mut collisions = Vec::new();
        let left = message_map(Side::Left, "INBOX", &left_pairs, &mut collisions);
        let right = message_map(Side::Right, "INBOX", &right_pairs, &mut collisions);
        assert!(collisions.is_empty());

        let hunks = diff_messages(
            "INBOX",
            &left,
            &right,
            &MessageSnapshots::new(),
            &MessageSnapshots::new(),
            perms_all(),
            perms_all(),
        );
        assert_eq!(hunks.len(), 1);
        assert!(matches!(
            &hunks[0],
            EmailHunk::Copy {
                source_side: Side::Left,
                target_side: Side::Right,
                mailbox,
                source_id,
                ..
            } if mailbox == "INBOX" && source_id == "L1"
        ));
    }

    #[test]
    fn diff_messages_right_added_emits_copy_to_left() {
        let left_pairs: EnvelopePairs = Vec::new();
        let right_pairs = vec![(1u64, envelope("R1", Some("<a>"), &[]))];
        let mut collisions = Vec::new();
        let left = message_map(Side::Left, "INBOX", &left_pairs, &mut collisions);
        let right = message_map(Side::Right, "INBOX", &right_pairs, &mut collisions);
        assert!(collisions.is_empty());

        let hunks = diff_messages(
            "INBOX",
            &left,
            &right,
            &MessageSnapshots::new(),
            &MessageSnapshots::new(),
            perms_all(),
            perms_all(),
        );
        assert_eq!(hunks.len(), 1);
        assert!(matches!(
            &hunks[0],
            EmailHunk::Copy {
                source_side: Side::Right,
                target_side: Side::Left,
                mailbox,
                source_id,
                ..
            } if mailbox == "INBOX" && source_id == "R1"
        ));
    }

    #[test]
    fn diff_messages_blocked_by_create_permission() {
        let left_pairs = vec![(1u64, envelope("L1", Some("<a>"), &[]))];
        let right_pairs: EnvelopePairs = Vec::new();
        let mut collisions = Vec::new();
        let left = message_map(Side::Left, "INBOX", &left_pairs, &mut collisions);
        let right = message_map(Side::Right, "INBOX", &right_pairs, &mut collisions);
        assert!(collisions.is_empty());

        let right_perms = perms_with(
            MailboxSidePermissions::default(),
            FlagSidePermissions { update: true },
            MessageSidePermissions {
                create: false,
                delete: true,
            },
        );

        let hunks = diff_messages(
            "INBOX",
            &left,
            &right,
            &MessageSnapshots::new(),
            &MessageSnapshots::new(),
            perms_all(),
            right_perms,
        );
        assert!(hunks.is_empty());
    }

    #[test]
    fn message_map_records_collision_keeps_first_seen() {
        let pairs = vec![
            (42u64, envelope("L1", Some("<dup>"), &[])),
            (42u64, envelope("L2", Some("<dup>"), &[])),
            (42u64, envelope("L3", Some("<dup>"), &[])),
        ];
        let mut collisions = Vec::new();
        let map = message_map(Side::Left, "INBOX", &pairs, &mut collisions);

        assert_eq!(map.len(), 1);
        assert_eq!(map[&42u64].id, "L1");

        assert_eq!(collisions.len(), 1);
        let c = &collisions[0];
        assert!(matches!(c.side, Side::Left));
        assert_eq!(c.mailbox, "INBOX");
        assert_eq!(c.message_id.as_deref(), Some("<dup>"));
        assert_eq!(c.ids, vec!["L1", "L2", "L3"]);
    }

    #[test]
    fn message_map_collision_without_message_id_uses_legacy_marker() {
        let pairs = vec![
            (7u64, envelope("A", None, &[])),
            (7u64, envelope("B", None, &[])),
        ];
        let mut collisions = Vec::new();
        let _ = message_map(Side::Right, "Sent", &pairs, &mut collisions);
        assert_eq!(collisions.len(), 1);
        assert!(collisions[0].message_id.is_none());
        assert_eq!(collisions[0].ids, vec!["A", "B"]);
    }

    #[test]
    fn diff_messages_continues_past_collision() {
        let left_pairs = vec![
            (42u64, envelope("L1", Some("<dup>"), &[])),
            (42u64, envelope("L2", Some("<dup>"), &[])),
            (99u64, envelope("L3", Some("<other>"), &[])),
        ];
        let right_pairs: EnvelopePairs = Vec::new();

        let mut collisions = Vec::new();
        let left = message_map(Side::Left, "INBOX", &left_pairs, &mut collisions);
        let right = message_map(Side::Right, "INBOX", &right_pairs, &mut collisions);

        let hunks = diff_messages(
            "INBOX",
            &left,
            &right,
            &MessageSnapshots::new(),
            &MessageSnapshots::new(),
            perms_all(),
            perms_all(),
        );
        assert_eq!(hunks.len(), 2);
        assert_eq!(collisions.len(), 1);
        let copied_ids: BTreeSet<String> = hunks
            .iter()
            .filter_map(|h| match h {
                EmailHunk::Copy { source_id, .. } => Some(source_id.clone()),
                _ => None,
            })
            .collect();
        assert!(copied_ids.contains("L1"));
        assert!(copied_ids.contains("L3"));
        assert!(!copied_ids.contains("L2"));
    }

    #[test]
    fn diff_flags_left_has_new_seen_emits_add_on_right() {
        let seen = Flag::from_iana(IanaFlag::Seen);
        let left = envelope("L1", Some("<a>"), &[seen.clone()]);
        let right = envelope("R1", Some("<a>"), &[]);

        let hunks = diff_flags(
            "INBOX",
            42,
            &left,
            &right,
            Some(&entry("L1", &[])),
            Some(&entry("R1", &[])),
            perms_all(),
            perms_all(),
        );
        assert_eq!(hunks.len(), 1);
        assert!(matches!(
            &hunks[0],
            EmailHunk::AddFlags { side: Side::Right, mailbox, id, flags, content_key: 42 }
                if mailbox == "INBOX" && id == "R1" && flags.contains(&seen)
        ));
    }

    #[test]
    fn diff_flags_right_removed_seen_emits_remove_on_left() {
        let seen = Flag::from_iana(IanaFlag::Seen);
        let left = envelope("L1", Some("<a>"), &[seen.clone()]);
        let right = envelope("R1", Some("<a>"), &[]);

        let hunks = diff_flags(
            "INBOX",
            42,
            &left,
            &right,
            Some(&entry("L1", &[seen.clone()])),
            Some(&entry("R1", &[seen.clone()])),
            perms_all(),
            perms_all(),
        );
        assert_eq!(hunks.len(), 1);
        assert!(matches!(
            &hunks[0],
            EmailHunk::RemoveFlags { side: Side::Left, mailbox, id, flags, content_key: 42 }
                if mailbox == "INBOX" && id == "L1" && flags.contains(&seen)
        ));
    }

    #[test]
    fn diff_flags_left_deleted_only_emits_delete_on_left() {
        let deleted = Flag::from_iana(IanaFlag::Deleted);
        let left = envelope("L1", Some("<a>"), &[deleted.clone()]);
        let right = envelope("R1", Some("<a>"), &[]);

        let hunks = diff_flags(
            "INBOX",
            42,
            &left,
            &right,
            None,
            None,
            perms_all(),
            perms_all(),
        );
        assert_eq!(hunks.len(), 1);
        assert!(matches!(
            &hunks[0],
            EmailHunk::Delete { side: Side::Left, mailbox, id, content_key: 42 }
                if mailbox == "INBOX" && id == "L1"
        ));
    }

    #[test]
    fn diff_flags_right_deleted_only_emits_delete_on_right() {
        let deleted = Flag::from_iana(IanaFlag::Deleted);
        let left = envelope("L1", Some("<a>"), &[]);
        let right = envelope("R1", Some("<a>"), &[deleted.clone()]);

        let hunks = diff_flags(
            "INBOX",
            42,
            &left,
            &right,
            None,
            None,
            perms_all(),
            perms_all(),
        );
        assert_eq!(hunks.len(), 1);
        assert!(matches!(
            &hunks[0],
            EmailHunk::Delete { side: Side::Right, mailbox, id, content_key: 42 }
                if mailbox == "INBOX" && id == "R1"
        ));
    }

    #[test]
    fn diff_flags_both_deleted_no_hunks() {
        let deleted = Flag::from_iana(IanaFlag::Deleted);
        let left = envelope("L1", Some("<a>"), &[deleted.clone()]);
        let right = envelope("R1", Some("<a>"), &[deleted.clone()]);

        let hunks = diff_flags(
            "INBOX",
            42,
            &left,
            &right,
            None,
            None,
            perms_all(),
            perms_all(),
        );
        assert!(hunks.is_empty());
    }
}
