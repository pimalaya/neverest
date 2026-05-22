//! Sync engine entry point.
//!
//! Wires [`crate::side::SideClient`] pairs into the bidirectional
//! 3-way merge described in the project plan:
//!
//! 1. resolve mailboxes on both sides, intersect under
//!    [`crate::config::MailboxFilter`];
//! 2. patch the mailbox set (creates only on v0; deletes are deferred
//!    to a future iteration since they're dangerous and rarely
//!    requested);
//! 3. per common mailbox, list envelopes both sides, derive the
//!    content key (see [`crate::sync::key::envelope_key`]), reconcile
//!    against the cached snapshot, and emit
//!    [`crate::sync::hunk::EmailHunk`]s for adds / flag patches /
//!    cache-driven deletes;
//! 4. apply hunks sequentially through
//!    [`io_email::client::EmailClientStd`]; collect errors per hunk;
//! 5. persist the post-sync snapshot to the per-account cache file.

use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::{Context, Result};
use io_email::client::EmailClientStd;
use io_email::envelope::Envelope;
use io_email::flag::Flag;

use crate::config::{
    AccountConfig, FlagSidePermissions, MailboxFilter, MailboxSidePermissions,
    MessageSidePermissions,
};
use crate::side::{Side, SideClient};
use crate::sync::cache::{CacheSnapshot, EnvelopeEntry, EnvelopeSnapshots, cache_path};
use crate::sync::event::{Handler, SyncEvent};
use crate::sync::hunk::{EmailHunk, MailboxHunk};
use crate::sync::key::envelope_key;
use crate::sync::report::SyncReport;

/// Per-side knobs the engine consults to decide whether a planned
/// hunk is allowed to materialize. Extracted from
/// [`crate::config::SideConfig`] up-front so the inner loops don't
/// re-walk the user config.
#[derive(Clone, Copy, Debug)]
pub struct SidePermissions {
    pub mailbox: MailboxSidePermissions,
    pub flag: FlagSidePermissions,
    pub message: MessageSidePermissions,
}

pub struct SyncBuilder {
    account_name: String,
    left_client: SideClient,
    right_client: SideClient,
    left_perms: SidePermissions,
    right_perms: SidePermissions,
    mailbox_filter: Option<MailboxFilter>,
    dry_run: bool,
}

impl SyncBuilder {
    pub fn new(
        account_name: impl Into<String>,
        account_config: &AccountConfig,
        left_client: SideClient,
        right_client: SideClient,
    ) -> Self {
        let left_perms = SidePermissions {
            mailbox: account_config.left.mailbox,
            flag: account_config.left.flag,
            message: account_config.left.message,
        };
        let right_perms = SidePermissions {
            mailbox: account_config.right.mailbox,
            flag: account_config.right.flag,
            message: account_config.right.message,
        };

        Self {
            account_name: account_name.into(),
            left_client,
            right_client,
            left_perms,
            right_perms,
            mailbox_filter: Some(account_config.mailbox.filters.clone()),
            dry_run: false,
        }
    }

    /// Overrides the configured mailbox filter with a CLI-supplied
    /// one. Pass `None` to fall back to the per-account default
    /// (already wired by `new`).
    pub fn with_mailbox_filter(mut self, filter: Option<MailboxFilter>) -> Self {
        if let Some(f) = filter {
            self.mailbox_filter = Some(f);
        }
        self
    }

    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    /// Runs the sync end-to-end. The returned [`SyncReport`] carries
    /// every applied hunk paired with its error (if any) — dry-run
    /// callers can read the same shape without any side effects on
    /// the backends.
    pub fn run(self, mut handler: impl Handler) -> Result<SyncReport> {
        let Self {
            account_name,
            left_client,
            right_client,
            left_perms,
            right_perms,
            mailbox_filter,
            dry_run,
        } = self;

        let cache_path = cache_path(&account_name)?;
        let mut snapshot = CacheSnapshot::load(&cache_path)?;

        let mut left = left_client.into_email_client();
        let mut right = right_client.into_email_client();

        // ── 1. list + filter mailboxes ───────────────────────────────────
        let left_mailboxes = list_mailbox_names(&mut left)?;
        let right_mailboxes = list_mailbox_names(&mut right)?;
        handler.handle(SyncEvent::ListedAllMailboxes)?;

        let filter = mailbox_filter.unwrap_or_default();
        let left_filtered = filter_mailboxes(&left_mailboxes, &filter);
        let right_filtered = filter_mailboxes(&right_mailboxes, &filter);

        // ── 2. compute + apply mailbox patch ─────────────────────────────
        let mailbox_hunks =
            diff_mailboxes(&left_filtered, &right_filtered, left_perms, right_perms);

        let mut report = SyncReport::default();
        if dry_run {
            for h in mailbox_hunks {
                report.mailbox.patch.push((h, None));
            }
        } else {
            for h in mailbox_hunks {
                let err = apply_mailbox_hunk(&mut left, &mut right, &h).err();
                report.mailbox.patch.push((h, err));
            }
        }
        handler.handle(SyncEvent::ProcessedAllMailboxHunks)?;

        // ── 3. envelope patch, per common mailbox ────────────────────────
        let common: BTreeSet<String> = left_filtered
            .intersection(&right_filtered)
            .cloned()
            .collect();

        let mut per_mailbox: HashMap<String, Vec<EmailHunk>> = HashMap::new();

        for mailbox in &common {
            let left_envs = left.list_envelopes(mailbox, None, None, false)?;
            let right_envs = right.list_envelopes(mailbox, None, None, false)?;

            let left_map = index_by_key(&left_envs);
            let right_map = index_by_key(&right_envs);

            let prev_left = snapshot
                .envelopes(Side::Left, mailbox)
                .cloned()
                .unwrap_or_default();
            let prev_right = snapshot
                .envelopes(Side::Right, mailbox)
                .cloned()
                .unwrap_or_default();

            let hunks = diff_envelopes(
                mailbox,
                &left_map,
                &right_map,
                &prev_left,
                &prev_right,
                left_perms,
                right_perms,
            );

            per_mailbox.insert(mailbox.clone(), hunks);
        }

        handler.handle(SyncEvent::GeneratedEmailPatch(per_mailbox.clone()))?;

        // ── 4. apply envelope hunks ──────────────────────────────────────
        for (mailbox, hunks) in &per_mailbox {
            for hunk in hunks {
                let err = if dry_run {
                    None
                } else {
                    apply_email_hunk(&mut left, &mut right, hunk)
                        .with_context(|| format!("apply hunk in `{mailbox}`"))
                        .err()
                };

                handler.handle(SyncEvent::ProcessedEmailHunk(hunk.clone()))?;
                report.email.patch.push((hunk.clone(), err));
            }
        }
        handler.handle(SyncEvent::ProcessedAllEmailHunks)?;

        // ── 5. persist post-sync snapshot ────────────────────────────────
        //
        // Re-list each common mailbox after the apply pass so the
        // snapshot reflects the LIVE post-patch state on both sides.
        // The extra round-trip costs one `list_envelopes` per side per
        // mailbox but is what lets the next run distinguish "user
        // deleted M between syncs" from "M was never on this side"
        // (without it, deletions resurrect on the next sync).
        if !dry_run {
            for mailbox in &common {
                let left_envs = left.list_envelopes(mailbox, None, None, false)?;
                let right_envs = right.list_envelopes(mailbox, None, None, false)?;
                snapshot.set_envelopes(
                    Side::Left,
                    mailbox.clone(),
                    to_snapshot(&index_by_key(&left_envs)),
                );
                snapshot.set_envelopes(
                    Side::Right,
                    mailbox.clone(),
                    to_snapshot(&index_by_key(&right_envs)),
                );
            }
            snapshot.save(&cache_path)?;
        }
        handler.handle(SyncEvent::Done)?;

        Ok(report)
    }
}

fn list_mailbox_names(client: &mut EmailClientStd) -> Result<HashSet<String>> {
    let mailboxes = client.list_mailboxes(false)?;
    Ok(mailboxes.into_iter().map(|m| m.name).collect())
}

fn filter_mailboxes(all: &HashSet<String>, filter: &MailboxFilter) -> HashSet<String> {
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

fn diff_mailboxes(
    left: &HashSet<String>,
    right: &HashSet<String>,
    left_perms: SidePermissions,
    right_perms: SidePermissions,
) -> Vec<MailboxHunk> {
    let mut hunks = Vec::new();
    for name in left.difference(right) {
        if right_perms.mailbox.create {
            hunks.push(MailboxHunk::Create {
                side: Side::Right,
                mailbox: name.clone(),
            });
        }
    }
    for name in right.difference(left) {
        if left_perms.mailbox.create {
            hunks.push(MailboxHunk::Create {
                side: Side::Left,
                mailbox: name.clone(),
            });
        }
    }
    hunks
}

fn apply_mailbox_hunk(
    _left: &mut EmailClientStd,
    _right: &mut EmailClientStd,
    hunk: &MailboxHunk,
) -> Result<()> {
    // io_email::client::EmailClientStd does not expose a public
    // create/delete-mailbox surface yet. Until that lands, surface a
    // descriptive error so the report shows what neverest could not
    // do without aborting the rest of the sync.
    anyhow::bail!("creating/deleting mailboxes is not yet wired through io-email: {hunk}")
}

type EnvelopeMap<'a> = HashMap<u64, &'a Envelope>;

fn index_by_key(envelopes: &[Envelope]) -> EnvelopeMap<'_> {
    envelopes
        .iter()
        .map(|env| (envelope_key(env), env))
        .collect()
}

fn to_snapshot(map: &EnvelopeMap<'_>) -> EnvelopeSnapshots {
    map.iter()
        .map(|(key, env)| {
            (
                key.to_string(),
                EnvelopeEntry {
                    id: env.id.clone(),
                    flags: env.flags.clone(),
                },
            )
        })
        .collect()
}

fn diff_envelopes(
    mailbox: &str,
    left: &EnvelopeMap<'_>,
    right: &EnvelopeMap<'_>,
    prev_left: &EnvelopeSnapshots,
    prev_right: &EnvelopeSnapshots,
    left_perms: SidePermissions,
    right_perms: SidePermissions,
) -> Vec<EmailHunk> {
    let mut hunks = Vec::new();

    for (key, env) in left {
        let key_str = key.to_string();
        match right.get(key) {
            Some(right_env) => {
                hunks.extend(diff_flags(mailbox, env, right_env, left_perms, right_perms));
            }
            None => {
                if prev_right.contains_key(&key_str) {
                    if left_perms.message.delete {
                        hunks.push(EmailHunk::Delete {
                            side: Side::Left,
                            mailbox: mailbox.to_string(),
                            id: env.id.clone(),
                        });
                    }
                } else if right_perms.message.create {
                    hunks.push(EmailHunk::Copy {
                        source_side: Side::Left,
                        target_side: Side::Right,
                        mailbox: mailbox.to_string(),
                        source_id: env.id.clone(),
                        flags: env.flags.clone(),
                    });
                }
            }
        }
    }

    for (key, env) in right {
        if left.contains_key(key) {
            continue; // already handled above
        }
        let key_str = key.to_string();
        if prev_left.contains_key(&key_str) {
            if right_perms.message.delete {
                hunks.push(EmailHunk::Delete {
                    side: Side::Right,
                    mailbox: mailbox.to_string(),
                    id: env.id.clone(),
                });
            }
        } else if left_perms.message.create {
            hunks.push(EmailHunk::Copy {
                source_side: Side::Right,
                target_side: Side::Left,
                mailbox: mailbox.to_string(),
                source_id: env.id.clone(),
                flags: env.flags.clone(),
            });
        }
    }

    hunks
}

fn diff_flags(
    mailbox: &str,
    left: &Envelope,
    right: &Envelope,
    left_perms: SidePermissions,
    right_perms: SidePermissions,
) -> Vec<EmailHunk> {
    let mut hunks = Vec::new();
    let to_add_right: BTreeSet<Flag> = left.flags.difference(&right.flags).copied().collect();
    let to_add_left: BTreeSet<Flag> = right.flags.difference(&left.flags).copied().collect();

    if !to_add_right.is_empty() && right_perms.flag.update {
        hunks.push(EmailHunk::AddFlags {
            side: Side::Right,
            mailbox: mailbox.to_string(),
            id: right.id.clone(),
            flags: to_add_right,
        });
    }
    if !to_add_left.is_empty() && left_perms.flag.update {
        hunks.push(EmailHunk::AddFlags {
            side: Side::Left,
            mailbox: mailbox.to_string(),
            id: left.id.clone(),
            flags: to_add_left,
        });
    }
    hunks
}

fn apply_email_hunk(
    left: &mut EmailClientStd,
    right: &mut EmailClientStd,
    hunk: &EmailHunk,
) -> Result<()> {
    match hunk {
        EmailHunk::Copy {
            source_side,
            target_side,
            mailbox,
            source_id,
            flags,
        } => {
            let (source, target) = pick_pair(left, right, *source_side, *target_side);
            let raw = source.get_message(mailbox, source_id)?;
            let flag_list: Vec<Flag> = flags.iter().copied().collect();
            target.add_message(mailbox, &flag_list, raw)?;
        }
        EmailHunk::AddFlags {
            side,
            mailbox,
            id,
            flags,
        } => {
            let client = pick(left, right, *side);
            let flag_list: Vec<Flag> = flags.iter().copied().collect();
            client.add_flags(mailbox, &[id.as_str()], &flag_list)?;
        }
        EmailHunk::Delete {
            side,
            mailbox,
            id: _,
        } => {
            // io_email currently has no delete-message op; surface a
            // descriptive error so the report shows what neverest
            // could not do without aborting the rest of the sync.
            let _ = pick(left, right, *side);
            let _ = mailbox;
            anyhow::bail!("deleting messages is not yet wired through io-email; hunk: {hunk}");
        }
    }
    Ok(())
}

fn pick<'a>(
    left: &'a mut EmailClientStd,
    right: &'a mut EmailClientStd,
    side: Side,
) -> &'a mut EmailClientStd {
    match side {
        Side::Left => left,
        Side::Right => right,
    }
}

fn pick_pair<'a>(
    left: &'a mut EmailClientStd,
    right: &'a mut EmailClientStd,
    source: Side,
    target: Side,
) -> (&'a mut EmailClientStd, &'a mut EmailClientStd) {
    match (source, target) {
        (Side::Left, Side::Right) => (left, right),
        (Side::Right, Side::Left) => (right, left),
        _ => unreachable!("Copy hunks always have distinct source/target sides"),
    }
}
