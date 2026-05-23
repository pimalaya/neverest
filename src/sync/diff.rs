//! Pure diff math.
//!
//! Functions in this module take snapshots and live listings, never
//! clients. Hunks emitted here are still ungated by the apply stage
//! and may fail at runtime; semantics gating (permissions) happens
//! up-front in each function via the [`SidePermissions`] arguments,
//! so the engine never re-walks the policy in its inner loops.
//!
//! See [`crate::sync::engine`] for the five-stage orchestration that
//! consumes these.

use std::collections::{BTreeSet, HashMap, HashSet, hash_map::Entry};

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
        key::message_key,
        report::MessageCollision,
    },
};

/// Map keyed by content hash from [`message_key`] so a `Copy` from one
/// side to the other lines up with the freshly added envelope on the
/// target side at the next sync.
pub type MessageMap<'a> = HashMap<u64, &'a Envelope>;

/// Owned `(content_key, Envelope)` pair list. The pair list is owned
/// by the per-mailbox scope in [`crate::sync::engine`]; the
/// [`message_map`] helper borrows from it to build a [`MessageMap`].
pub type EnvelopePairs = Vec<(u64, Envelope)>;

/// Re-keys a live envelope listing by content hash. Used on the
/// `FullListRequired` / `UnsupportedOperation` fall-back path; the
/// incremental path constructs pairs through
/// [`pairs_from_delta`] which preserves the per-message key already
/// stored in the cache.
pub fn pairs_from_envelopes(messages: Vec<Envelope>) -> EnvelopePairs {
    messages.into_iter().map(|m| (message_key(&m), m)).collect()
}

/// Borrows an [`EnvelopePairs`] view into a [`MessageMap`] without
/// re-computing keys. On a content-key collision the first envelope
/// wins (so the diff still applies to it normally); duplicates are
/// appended to `collisions` and skipped for this sync. Aborting the
/// whole mailbox on collision would be too aggressive: with
/// Message-ID-first keying a clash usually means a single RFC 5322
/// violation on one message, which should not stop the rest of the
/// mailbox from syncing.
pub fn message_map<'a>(
    side: Side,
    mailbox: &str,
    pairs: &'a EnvelopePairs,
    collisions: &mut Vec<MessageCollision>,
) -> MessageMap<'a> {
    let mut out: MessageMap<'a> = HashMap::with_capacity(pairs.len());
    // Group dups by key so a triple-collision surfaces as one report
    // entry listing all three ids instead of two entries.
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

/// Projects an [`EnvelopePairs`] view into the per-mailbox snapshot
/// shape persisted by the cache. Used to capture the current sync's
/// pre-apply state inline (no follow-up `list_envelopes`); the next
/// sync's three-way diff consumes it as `prev_*`. The pairs already
/// match the snapshot's keying (content hash) so this is a pure
/// re-shape.
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

/// Synthesizes an [`EnvelopePairs`] for a side that returned
/// [`io_email::envelope::EnvelopeDiff::Incremental`]. The prior
/// snapshot supplies the still-present messages (keyed by content
/// hash); `vanished_ids` removes them, `flag_updates` overrides their
/// flags, and `new_envelopes` adds the messages added since the
/// cached checkpoint. The surviving snapshot entries get stub
/// envelopes (empty subject/from/date/size); the diff/apply paths
/// only consult `id` and `flags`, so the elision is safe.
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

/// An envelope shell with just `id` + `flags` populated; the rest is
/// default-empty. Used to bridge snapshot-only entries (the cache
/// does not store subject/from/date/size) into [`MessageMap`] so the
/// diff/apply paths can address them.
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

/// Mailbox-level three-way diff.
///
/// A name that appears on one side and not the other is classified by
/// consulting the cached snapshot's last-known mailbox set per side:
///   * absent from the other side AND absent from the other side's
///     snapshot: this side just added it, create on the other side
///     (gated by the other side's `mailbox.create` permission);
///   * absent from the other side BUT present in the other side's
///     snapshot: the other side deleted it, propagate the deletion
///     to this side (gated by this side's `mailbox.delete` permission).
///
/// Without a snapshot (first sync) the diff degenerates to "every
/// asymmetry is an add", matching what neverest did before three-way
/// merge landed.
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

/// Message-level three-way diff for one mailbox.
///
/// Emits `Copy`/`Delete` based on presence in left × right × snapshot
/// per side. Flag-only divergences (message present on both sides)
/// delegate to [`diff_flags`].
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

/// Flag-level diff for a pair of messages that exist on both sides.
///
/// Three-way against the cached snapshot when available: a flag
/// present on one side and absent on the other is interpreted as an
/// *add* on the side that has it when neither snapshot recorded it,
/// and as a *remove* on the side that lacks it when the snapshot
/// recorded it on the side that no longer has it. Without snapshot
/// entries the diff reverts to union-add semantics (matches first-sync
/// behaviour).
///
/// `\Deleted` is treated as a sync verb (per the architecture
/// decisions): when one side carries it and the other does not, the
/// engine emits a `delete_message` on the side that does NOT carry it
/// rather than propagating the flag across.
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

    // ── filter_mailboxes ────────────────────────────────────────────

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

    // ── diff_mailboxes ──────────────────────────────────────────────

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
        // Left still has INBOX; right no longer has it but did at
        // last sync. Delete on left.
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
        // Right wants left to delete but left.mailbox.delete = false.
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

    // ── diff_messages ───────────────────────────────────────────────

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
        // Left has message X, right lacks it, right.message.create = false.
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

        // First-seen survives in the map so the diff still has
        // something to reconcile.
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
        // Two envelopes that share a legacy key (no Message-ID); the
        // recorded collision's `message_id` field is `None`.
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
        // Two colliding envelopes on left plus one unrelated envelope
        // on left and right; the engine should still emit a hunk for
        // the unrelated message even though L1/L2 collide.
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
        // Two Copy hunks: one for L1 (kept first-seen at key 42) and
        // one for L3. L2 is parked in the collisions list.
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

    // ── diff_flags ──────────────────────────────────────────────────

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

        // Both sides previously had seen; the right snapshot still
        // records seen, so the divergence resolves to "right removed
        // seen → mirror the removal on left".
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
