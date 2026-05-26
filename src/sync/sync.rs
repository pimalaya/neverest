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

//! Bidirectional 3-way sync `run` entry point: drives the mailbox /
//! message patch over both sides against the cached snapshot.

use std::{
    collections::{BTreeSet, HashSet},
    thread,
};

use anyhow::{Result, anyhow};
use io_email::{
    client::{EmailClientStd, EmailClientStdError},
    envelope::EnvelopeDiff,
    mailbox::MailboxDiff,
};
use log::{debug, warn};
use pimalaya_cli::spinner::Spinner;

use crate::{
    config::{AccountConfig, MailboxFilter},
    side::Side,
    sync::{
        cache::{CacheSnapshot, MessageEntry},
        diff::{
            EnvelopePairs, diff_mailboxes, diff_messages, filter_mailboxes, message_map,
            pairs_from_delta, pairs_from_envelopes, pairs_to_snapshot,
        },
        hunk::{EmailHunk, MailboxHunk},
        pool::{HunkOutcome, MailboxHunkOutcome, Pool},
        report::{PatchEntry, SyncReport},
    },
};

/// Issues a SELECT on `client` when its backend is IMAP, otherwise a
/// no-op; primes workers for an `auto_select=false` hunk batch.
#[cfg(feature = "imap")]
fn imap_select(client: &mut EmailClientStd, mailbox: &str) -> Result<()> {
    if let Some(imap) = client.as_imap_mut() {
        imap.select(mailbox.to_owned().try_into()?)?;
    }
    Ok(())
}

/// Probes the per-side mailbox set; uses `diff_mailboxes` when supported
/// and falls back to a full `list_mailboxes` otherwise.
fn probe_side_mailboxes(
    client: &mut EmailClientStd,
    side: Side,
    snapshot: &CacheSnapshot,
) -> Result<(HashSet<String>, Option<Vec<u8>>)> {
    let cached = snapshot.mailbox_state(side);

    let (unchanged, new_state) = match client.diff_mailboxes(cached) {
        Ok(MailboxDiff::Unchanged { new_state }) => (true, Some(new_state)),
        Ok(MailboxDiff::Changed { new_state }) => (false, new_state),
        Err(EmailClientStdError::UnsupportedOperation) => (false, None),
        Err(err) => {
            warn!("{side} diff_mailboxes failed: {err:#}");
            (false, None)
        }
    };

    let mailboxes: HashSet<String> = if unchanged {
        debug!("{side} mailbox set unchanged, reusing snapshot");
        snapshot.mailbox_names(side)
    } else {
        debug!("listing {side} mailboxes");
        client
            .list_mailboxes(false)?
            .into_iter()
            .map(|m| m.name)
            .collect()
    };

    Ok((mailboxes, new_state))
}

/// Resolves the envelope set for `(side, mailbox)`; uses the incremental
/// diff fast path when available, otherwise falls back to a full
/// `list_envelopes`.
fn fetch_side_envelopes(
    client: &mut EmailClientStd,
    side: Side,
    mailbox: &str,
    snapshot: &CacheSnapshot,
) -> Result<(EnvelopePairs, Option<Vec<u8>>)> {
    let diff = resolve_diff(client, side, mailbox, snapshot);

    match diff {
        Ok(EnvelopeDiff::Incremental {
            new_state,
            flag_updates,
            new_envelopes,
            vanished_ids,
        }) => {
            debug!(
                "{side} `{mailbox}`: {}+ {}~ {}- (delta)",
                new_envelopes.len(),
                flag_updates.len(),
                vanished_ids.len(),
            );
            let prev = snapshot
                .messages(side, mailbox)
                .cloned()
                .unwrap_or_default();
            let vanished: HashSet<String> = vanished_ids.into_iter().collect();
            let pairs = pairs_from_delta(&prev, flag_updates, new_envelopes, vanished);
            let captured = (!new_state.is_empty()).then_some(new_state);
            Ok((pairs, captured))
        }
        Ok(EnvelopeDiff::FullListRequired { new_state }) => {
            let msgs = client.list_envelopes(mailbox, None, None, false)?;
            Ok((pairs_from_envelopes(msgs), new_state))
        }
        Err(err) => {
            let unsupported = matches!(
                err.downcast_ref::<EmailClientStdError>(),
                Some(EmailClientStdError::UnsupportedOperation),
            );
            if !unsupported {
                warn!("{side} diff_envelopes `{mailbox}` failed: {err:#}");
            }
            let msgs = client.list_envelopes(mailbox, None, None, false)?;
            Ok((pairs_from_envelopes(msgs), None))
        }
    }
}

/// Routes the envelope diff to the matching backend: snapshot-driven
/// for m2dir, protocol checkpoint for IMAP / JMAP.
fn resolve_diff(
    client: &mut EmailClientStd,
    side: Side,
    mailbox: &str,
    snapshot: &CacheSnapshot,
) -> Result<EnvelopeDiff> {
    #[cfg(feature = "m2dir")]
    if client.as_m2dir().is_some() {
        let prev = snapshot.messages(side, mailbox);
        return crate::sync::diff::diff_envelopes(client, mailbox, prev);
    }
    let cached = snapshot.state(side, mailbox);
    client.diff_envelopes(mailbox, cached).map_err(Into::into)
}

/// Folds a successful hunk apply into the pre-apply snapshot baseline.
fn update_snapshot_from_hunk(
    snapshot: &mut CacheSnapshot,
    mailbox: &str,
    hunk: &EmailHunk,
    target_id: Option<String>,
) {
    match hunk {
        EmailHunk::Copy {
            target_side,
            flags,
            content_key,
            ..
        } => {
            // NOTE: Copy should always surface the new id on success;
            // skip silently if absent rather than panic.
            let Some(id) = target_id else {
                return;
            };
            let snap = snapshot.messages_mut(*target_side, mailbox);
            snap.insert(
                content_key.to_string(),
                MessageEntry {
                    id,
                    flags: flags.clone(),
                },
            );
        }
        EmailHunk::Delete {
            side, content_key, ..
        } => {
            let snap = snapshot.messages_mut(*side, mailbox);
            snap.remove(&content_key.to_string());
        }
        EmailHunk::AddFlags {
            side,
            content_key,
            flags,
            ..
        } => {
            let snap = snapshot.messages_mut(*side, mailbox);
            if let Some(entry) = snap.get_mut(&content_key.to_string()) {
                entry.flags.extend(flags.iter().cloned());
            }
        }
        EmailHunk::RemoveFlags {
            side,
            content_key,
            flags,
            ..
        } => {
            let snap = snapshot.messages_mut(*side, mailbox);
            if let Some(entry) = snap.get_mut(&content_key.to_string()) {
                for flag in flags {
                    entry.flags.remove(flag);
                }
            }
        }
    }
}

/// Runs the sync end-to-end and returns a [`SyncReport`] pairing every
/// applied hunk with its error (if any).
pub fn run(
    account_name: impl Into<String>,
    account_config: &AccountConfig,
    mut pool: Pool,
    mailbox_filter: Option<MailboxFilter>,
    dry_run: bool,
) -> Result<SyncReport> {
    let account_name = account_name.into();
    let left_perms = account_config.left.permissions();
    let right_perms = account_config.right.permissions();

    let mailbox_filter = mailbox_filter.unwrap_or_else(|| account_config.mailbox.filters.clone());

    let mut report = SyncReport {
        account: account_name.clone(),
        dry_run,
        ..Default::default()
    };

    let cache_path = CacheSnapshot::path(&account_name)?;
    let mut snapshot = CacheSnapshot::load(&cache_path)?;

    // 1. list + filter mailboxes (left and right probed in parallel).
    let s = Spinner::start("Listing mailboxes…");

    let (left_outcome, right_outcome) = thread::scope(|scope| -> Result<_> {
        let left_client = &mut pool.left[0];
        let right_client = &mut pool.right[0];
        let snap = &snapshot;

        let lh = scope.spawn(move || probe_side_mailboxes(left_client, Side::Left, snap));
        let rh = scope.spawn(move || probe_side_mailboxes(right_client, Side::Right, snap));
        let left = lh
            .join()
            .map_err(|_| anyhow!("Left mailbox probe panicked"))?;
        let right = rh
            .join()
            .map_err(|_| anyhow!("Right mailbox probe panicked"))?;
        Ok((left, right))
    })?;

    let (left_mailboxes, left_mailbox_state) = left_outcome?;
    let (right_mailboxes, right_mailbox_state) = right_outcome?;

    if let Some(state) = left_mailbox_state {
        snapshot.set_mailbox_state(Side::Left, state);
    }
    if let Some(state) = right_mailbox_state {
        snapshot.set_mailbox_state(Side::Right, state);
    }

    s.success(format!(
        "Listed mailboxes ({} left, {} right)",
        left_mailboxes.len(),
        right_mailboxes.len()
    ));

    let left_filtered = filter_mailboxes(&left_mailboxes, &mailbox_filter);
    let right_filtered = filter_mailboxes(&right_mailboxes, &mailbox_filter);

    // 2. compute + apply mailbox patch (fanned out across worker pairs).
    let prev_left_mailboxes = snapshot.mailbox_names(Side::Left);
    let prev_right_mailboxes = snapshot.mailbox_names(Side::Right);
    let mailbox_hunks = diff_mailboxes(
        &left_filtered,
        &right_filtered,
        &prev_left_mailboxes,
        &prev_right_mailboxes,
        left_perms,
        right_perms,
    );

    debug!(
        "mailbox patch: {} hunks{}",
        mailbox_hunks.len(),
        if dry_run { " (dry-run)" } else { "" }
    );

    let mailbox_hunk_count = mailbox_hunks.len();
    if mailbox_hunk_count > 0 {
        let s = Spinner::start(format!("Patching {mailbox_hunk_count} mailbox hunks…"));
        if dry_run {
            for h in mailbox_hunks {
                report.mailbox.patch.push(PatchEntry::new(h, None));
            }
        } else {
            let outcomes = pool.apply_mailbox_hunks(mailbox_hunks, |_, _| {})?;
            for MailboxHunkOutcome { hunk, result } in outcomes {
                report
                    .mailbox
                    .patch
                    .push(PatchEntry::new(hunk, result.err()));
            }
        }
        s.success(format!(
            "Patched {mailbox_hunk_count} mailbox hunks{}",
            if dry_run { " (dry-run)" } else { "" }
        ));
    }

    // 3. message patch, per common mailbox (post-stage-2 set: keep
    //    just-created mailboxes, drop just-deleted ones).
    let mut common: BTreeSet<String> = left_filtered
        .intersection(&right_filtered)
        .cloned()
        .collect();
    for entry in &report.mailbox.patch {
        let applied = dry_run || entry.error.is_none();
        if !applied {
            continue;
        }

        match &entry.hunk {
            MailboxHunk::Create { mailbox, .. } => {
                common.insert(mailbox.clone());
            }
            MailboxHunk::Delete { mailbox, .. } => {
                common.remove(mailbox);
            }
        }
    }

    let total_mailboxes = common.len();

    if total_mailboxes == 0 {
        let filter_kind = match &mailbox_filter {
            MailboxFilter::All => "all",
            MailboxFilter::Include(_) => "include",
            MailboxFilter::Exclude(_) => "exclude",
        };
        debug!(
            "no common mailbox after `{filter_kind}` filter ({}L / {}R)",
            left_filtered.len(),
            right_filtered.len(),
        );
    }

    for (index, mailbox) in common.iter().enumerate() {
        let position = index + 1;
        let prefix = format!("[{position}/{total_mailboxes}] Syncing {mailbox}");
        let s = Spinner::start(format!("{prefix} (0%)"));
        debug!("resolving `{mailbox}` on both sides");

        let left_present = left_filtered.contains(mailbox);
        let right_present = right_filtered.contains(mailbox);
        let mailbox_str = mailbox.as_str();

        let (left_fetch, right_fetch) = thread::scope(|scope| -> Result<_> {
            let left_client = &mut pool.left[0];
            let right_client = &mut pool.right[0];
            let snap = &snapshot;

            let lh = scope.spawn(move || -> Result<(EnvelopePairs, Option<Vec<u8>>)> {
                if left_present {
                    fetch_side_envelopes(left_client, Side::Left, mailbox_str, snap)
                } else {
                    Ok((Vec::new(), None))
                }
            });
            let rh = scope.spawn(move || -> Result<(EnvelopePairs, Option<Vec<u8>>)> {
                if right_present {
                    fetch_side_envelopes(right_client, Side::Right, mailbox_str, snap)
                } else {
                    Ok((Vec::new(), None))
                }
            });
            let left = lh
                .join()
                .map_err(|_| anyhow!("Left envelope fetch panicked"))?;
            let right = rh
                .join()
                .map_err(|_| anyhow!("Right envelope fetch panicked"))?;
            Ok((left, right))
        })?;

        let (left_pairs, left_state) = left_fetch?;
        let (right_pairs, right_state) = right_fetch?;

        let left_map = message_map(Side::Left, mailbox, &left_pairs, &mut report.collisions);
        let right_map = message_map(Side::Right, mailbox, &right_pairs, &mut report.collisions);

        let prev_left = snapshot
            .messages(Side::Left, mailbox)
            .cloned()
            .unwrap_or_default();
        let prev_right = snapshot
            .messages(Side::Right, mailbox)
            .cloned()
            .unwrap_or_default();

        let hunks = diff_messages(
            mailbox,
            &left_map,
            &right_map,
            &prev_left,
            &prev_right,
            left_perms,
            right_perms,
        );

        // NOTE: capture the pre-apply baseline now; the outcome loop
        // below folds each successful hunk into it so stage 4 persists
        // the post-apply state.
        if !dry_run {
            snapshot.set_messages(Side::Left, mailbox.clone(), pairs_to_snapshot(&left_pairs));
            snapshot.set_messages(
                Side::Right,
                mailbox.clone(),
                pairs_to_snapshot(&right_pairs),
            );
            if let Some(state) = left_state {
                snapshot.set_state(Side::Left, mailbox.clone(), state);
            }
            if let Some(state) = right_state {
                snapshot.set_state(Side::Right, mailbox.clone(), state);
            }
        }

        let total = hunks.len();
        if total == 0 {
            s.clear();
            continue;
        }

        debug!("applying {total} hunks in `{mailbox}`");

        if dry_run {
            for hunk in hunks {
                report.email.patch.push(PatchEntry::new(hunk, None));
            }
        } else {
            // NOTE: pre-select on every pool client in parallel so per-op
            // IMAP wrappers (running with `auto_select=false`) skip their
            // own SELECT.
            #[cfg(feature = "imap")]
            thread::scope(|scope| -> Result<()> {
                let mut handles = Vec::new();
                if left_present {
                    for client in pool.left.iter_mut() {
                        handles.push(scope.spawn(move || imap_select(client, mailbox_str)));
                    }
                }
                if right_present {
                    for client in pool.right.iter_mut() {
                        handles.push(scope.spawn(move || imap_select(client, mailbox_str)));
                    }
                }
                for h in handles {
                    h.join()
                        .map_err(|_| anyhow!("IMAP pre-select worker panicked"))??;
                }
                Ok(())
            })?;

            let outcomes = pool.apply_in_mailbox(mailbox, hunks, |applied, total| {
                let percent = (applied * 100) / total.max(1);
                s.set_message(format!("{prefix} ({percent}%)"));
            })?;
            for outcome in outcomes {
                let HunkOutcome { hunk, result } = outcome;
                match result {
                    Ok(target_id) => {
                        update_snapshot_from_hunk(&mut snapshot, mailbox, &hunk, target_id);
                        report.email.patch.push(PatchEntry::new(hunk, None));
                    }
                    Err(err) => {
                        report.email.patch.push(PatchEntry::new(hunk, Some(err)));
                    }
                }
            }
        }

        s.success(format!(
            "{mailbox}: {total} message hunks{}",
            if dry_run { " (dry-run)" } else { "" }
        ));
    }

    // 4. persist post-sync snapshot.
    if !dry_run {
        let s = Spinner::start("Persisting snapshot…");
        debug!("persisting snapshot at `{}`", cache_path.display());
        snapshot.record(&report.mailbox.patch, &cache_path)?;
        s.success("Persisted snapshot");
    }

    Ok(report)
}
