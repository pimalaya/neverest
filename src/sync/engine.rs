//! Sync engine entry point.
//!
//! Drives the bidirectional 3-way merge through
//! [`io_email::client::EmailClientStd`] on both sides:
//!
//! 1. resolve mailboxes on both sides, three-way against the cached
//!    snapshot under [`crate::config::MailboxFilter`];
//! 2. patch the mailbox set (create on the side missing a name the
//!    other side has and the snapshot never recorded; delete on the
//!    side still keeping a name the other side removed since the last
//!    sync, subject to per-side permissions);
//! 3. per common mailbox (post-patch), list messages on both sides,
//!    derive the content key (see [`crate::sync::key::message_key`]),
//!    reconcile against the cached snapshot, and emit
//!    [`crate::sync::hunk::EmailHunk`]s for copies, flag adds/removes,
//!    and cache-driven deletes (a Deleted-flag on one side routes to a
//!    `delete_message` on the other side rather than propagating);
//! 4. apply hunks: mailbox hunks serial on `clients[0]` of each pool,
//!    message hunks fanned out across [`crate::sync::apply::PairedPools`];
//! 5. persist the post-sync snapshot via [`CacheSnapshot::record`].

use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::Result;
use io_email::{
    client::{EmailClientStd, EmailClientStdError},
    envelope::EnvelopeDiff,
    mailbox::MailboxDiff,
};
use log::{debug, info, warn};
use pimalaya_cli::spinner::Spinner;

use crate::{
    config::{AccountConfig, MailboxFilter},
    side::Side,
    sync::{
        apply::{HunkOutcome, PairedPools},
        cache::{CacheSnapshot, MessageEntry, cache_path},
        diff::{
            EnvelopePairs, diff_mailboxes, diff_messages, filter_mailboxes, message_map,
            pairs_from_delta, pairs_from_envelopes, pairs_to_snapshot,
        },
        hunk::{EmailHunk, MailboxHunk},
        pool::SidePool,
        report::{PatchEntry, SyncReport},
    },
};

/// Outcome of the per-side mailbox-set probe. `Unchanged` lets the
/// engine reuse `snapshot.mailbox_names(side)` and skip the
/// `list_mailboxes` round-trip; `Changed` falls through to the full
/// list. The status is collapsed locally; the new mailbox-set state
/// (if any) lands in `new_mailbox_states` for stage 4b.
enum MailboxStatus {
    Unchanged,
    Changed,
}

/// Probes [`EmailClientStd::diff_mailboxes`] for `side`. Returns
/// `Unchanged` only when the backend explicitly reports it (JMAP with
/// a matching `Mailbox/state`); `Changed`, `UnsupportedOperation`, or
/// any error fall through to the listing path. Captures the new
/// state bytes when present.
fn probe_mailbox_diff(
    client: &mut EmailClientStd,
    side: Side,
    snapshot: &CacheSnapshot,
    out: &mut HashMap<Side, Vec<u8>>,
) -> MailboxStatus {
    let cached = snapshot.mailbox_state(side);

    match client.diff_mailboxes(cached) {
        Ok(MailboxDiff::Unchanged { new_state }) => {
            out.insert(side, new_state);
            MailboxStatus::Unchanged
        }
        Ok(MailboxDiff::Changed {
            new_state: Some(bytes),
        }) => {
            out.insert(side, bytes);
            MailboxStatus::Changed
        }
        Ok(MailboxDiff::Changed { new_state: None }) => MailboxStatus::Changed,
        Err(EmailClientStdError::UnsupportedOperation) => MailboxStatus::Changed,
        Err(err) => {
            warn!("{side} diff_mailboxes failed: {err:#}");
            MailboxStatus::Changed
        }
    }
}

/// Resolves the envelope set for `(side, mailbox)`. Tries the
/// incremental [`EmailClientStd::diff_envelopes`] fast path first;
/// when the backend returns
/// [`io_email::envelope::EnvelopeDiff::Incremental`], synthesizes
/// the per-message [`crate::sync::diff::EnvelopePairs`] from the
/// cached snapshot plus the delta and skips the full
/// `list_envelopes` round-trip. Falls back to `list_envelopes` on
/// [`io_email::envelope::EnvelopeDiff::FullListRequired`],
/// `UnsupportedOperation`, or any transient error. Captures the new
/// state into `states_out` so the engine folds it into the snapshot
/// at stage 4b.
fn fetch_side_envelopes(
    client: &mut EmailClientStd,
    side: Side,
    mailbox: &str,
    snapshot: &CacheSnapshot,
    states_out: &mut HashMap<(Side, String), Vec<u8>>,
) -> Result<EnvelopePairs> {
    let diff = resolve_diff(client, side, mailbox, snapshot);

    match diff {
        Ok(EnvelopeDiff::Incremental {
            new_state,
            flag_updates,
            new_envelopes,
            vanished_ids,
        }) => {
            if !new_state.is_empty() {
                states_out.insert((side, mailbox.to_string()), new_state);
            }
            debug!(
                "{side} `{mailbox}` incremental: {} flag updates, {} new, {} vanished",
                flag_updates.len(),
                new_envelopes.len(),
                vanished_ids.len(),
            );
            let prev = snapshot
                .messages(side, mailbox)
                .cloned()
                .unwrap_or_default();
            let vanished: HashSet<String> = vanished_ids.into_iter().collect();
            Ok(pairs_from_delta(
                &prev,
                flag_updates,
                new_envelopes,
                vanished,
            ))
        }
        Ok(EnvelopeDiff::FullListRequired { new_state }) => {
            if let Some(bytes) = new_state {
                states_out.insert((side, mailbox.to_string()), bytes);
            }
            let msgs = client.list_envelopes(mailbox, None, None, false)?;
            Ok(pairs_from_envelopes(msgs))
        }
        Err(err) if is_unsupported(&err) => {
            let msgs = client.list_envelopes(mailbox, None, None, false)?;
            Ok(pairs_from_envelopes(msgs))
        }
        Err(err) => {
            warn!("{side} diff_envelopes `{mailbox}` failed: {err:#}");
            let msgs = client.list_envelopes(mailbox, None, None, false)?;
            Ok(pairs_from_envelopes(msgs))
        }
    }
}

/// Routes the diff to the right backend.
///
/// m2dir: snapshot-driven (the persisted [`MessageSnapshots`] for the
/// side/mailbox is the state; no separate `state` blob is used). On a
/// fresh mailbox `prev` is `None`, the listing returns every entry as
/// a new envelope and `pairs_from_delta` builds the initial baseline.
///
/// IMAP/JMAP: the LCD `diff_envelopes` is fed the cached protocol
/// state token (QRESYNC checkpoint / `Email/state`); the wire delta
/// carries flag updates, new envelopes and vanished ids since that
/// token.
fn resolve_diff(
    client: &mut EmailClientStd,
    side: Side,
    mailbox: &str,
    snapshot: &CacheSnapshot,
) -> Result<EnvelopeDiff> {
    #[cfg(feature = "m2dir")]
    if client.as_m2dir().is_some() {
        let prev = snapshot.messages(side, mailbox);
        return crate::sync::m2dir_diff::diff_envelopes(client, mailbox, prev);
    }
    let cached = snapshot.state(side, mailbox);
    client.diff_envelopes(mailbox, cached).map_err(Into::into)
}

/// Mutates the in-memory snapshot for the just-applied hunk.
///
/// The pre-apply baseline is captured by `pairs_to_snapshot`; this
/// folds each successful apply into that baseline so the persisted
/// snapshot reflects the post-apply state of both sides. The next
/// sync's diff (m2dir's snapshot-driven path; IMAP/JMAP's
/// `pairs_from_delta`) treats this as the baseline and only reports
/// what actually changed afterwards.
///
/// Failed hunks are not folded in: their pre-apply baseline entries
/// stay correct (the apply was a no-op on the backend) and the next
/// sync will re-detect the divergence.
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
            let Some(id) = target_id else {
                // Copy should always surface the new id on success;
                // skip silently if absent rather than panic.
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

/// Recognises [`EmailClientStdError::UnsupportedOperation`] wrapped
/// inside an `anyhow::Error`, so the caller can drop to the full
/// `list_envelopes` path for backends without a diff capability.
fn is_unsupported(err: &anyhow::Error) -> bool {
    matches!(
        err.downcast_ref::<EmailClientStdError>(),
        Some(EmailClientStdError::UnsupportedOperation)
    )
}

/// Runs the sync end-to-end. Returns a [`SyncReport`] that carries
/// every applied hunk paired with its error (if any); the report is
/// `Display + Serialize` so the caller hands it straight to
/// [`pimalaya_cli::printer::Printer::out`].
///
/// Mailboxes are processed serially; within a mailbox the message
/// hunks fan out across a thread pool sized to
/// [`PairedPools::worker_count`]. Each worker owns one left client and
/// one right client for the duration of the mailbox so `Copy` hunks
/// (which read from one side and write to the other) always have both
/// ends in hand. The serial-only stages (mailbox list, mailbox patch,
/// message listing, cache snapshot) run on `pool.left[0]` /
/// `pool.right[0]`.
///
/// Progress is surfaced via `log::info!` (stage transitions,
/// per-mailbox start) and `log::debug!` (per-hunk apply). No event
/// callback or spinner is involved.
pub fn run(
    account_name: impl Into<String>,
    account_config: &AccountConfig,
    left: SidePool,
    right: SidePool,
    cli_mailbox_filter: Option<MailboxFilter>,
    dry_run: bool,
) -> Result<SyncReport> {
    let account_name = account_name.into();
    let left_perms = account_config.left.permissions();
    let right_perms = account_config.right.permissions();

    let mailbox_filter =
        cli_mailbox_filter.unwrap_or_else(|| account_config.mailbox.filters.clone());

    let mut report = SyncReport {
        account: account_name.clone(),
        dry_run,
        ..Default::default()
    };

    let mut pool = PairedPools {
        left: left.into_clients(),
        right: right.into_clients(),
    };

    let cache_path = cache_path(&account_name)?;
    let mut snapshot = CacheSnapshot::load(&cache_path)?;

    // 1. list + filter mailboxes
    //
    // Probe each side's mailbox-set checkpoint first; when both sides
    // report `Unchanged`, we can skip the full `list_mailboxes`
    // round-trip and reuse the cached mailbox names from the
    // snapshot. Captured states land in `new_mailbox_states` and fold
    // into the snapshot at stage 4b.
    let s = Spinner::start("Listing mailboxes…");

    let mut new_mailbox_states: HashMap<Side, Vec<u8>> = HashMap::new();
    let left_mb_status = probe_mailbox_diff(
        &mut pool.left[0],
        Side::Left,
        &snapshot,
        &mut new_mailbox_states,
    );
    let right_mb_status = probe_mailbox_diff(
        &mut pool.right[0],
        Side::Right,
        &snapshot,
        &mut new_mailbox_states,
    );

    let left_mailboxes: HashSet<String> = if matches!(left_mb_status, MailboxStatus::Unchanged) {
        info!("left mailbox set unchanged since last sync, reusing snapshot");
        snapshot.mailbox_names(Side::Left)
    } else {
        info!("listing mailboxes on left side");
        pool.left[0]
            .list_mailboxes(false)?
            .into_iter()
            .map(|m| m.name)
            .collect()
    };
    let right_mailboxes: HashSet<String> = if matches!(right_mb_status, MailboxStatus::Unchanged) {
        info!("right mailbox set unchanged since last sync, reusing snapshot");
        snapshot.mailbox_names(Side::Right)
    } else {
        info!("listing mailboxes on right side");
        pool.right[0]
            .list_mailboxes(false)?
            .into_iter()
            .map(|m| m.name)
            .collect()
    };

    s.success(format!(
        "Listed mailboxes: {} left, {} right",
        left_mailboxes.len(),
        right_mailboxes.len()
    ));

    let left_filtered = filter_mailboxes(&left_mailboxes, &mailbox_filter);
    let right_filtered = filter_mailboxes(&right_mailboxes, &mailbox_filter);

    // 2. compute + apply mailbox patch
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

    info!(
        "computed mailbox patch: {} hunks{}",
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
            for h in mailbox_hunks {
                debug!("applying mailbox hunk: {h}");
                let err = h.apply(&mut pool.left[0], &mut pool.right[0]).err();
                report.mailbox.patch.push(PatchEntry::new(h, err));
            }
        }
        s.success(format!(
            "Patched {mailbox_hunk_count} mailbox hunks{}",
            if dry_run { " (dry-run)" } else { "" }
        ));
    }

    // 3. message patch, per common mailbox
    //
    // `common` is the post-stage-2 mailbox set: the original
    // intersection, plus mailboxes that were just created (so their
    // messages flow through stage 3 in the same sync), minus
    // mailboxes that were just deleted (no point listing them). In
    // dry-run we optimistically assume every planned hunk would have
    // applied cleanly so the preview shows the full would-be patch.
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

    // 3 + 4. per common mailbox: resolve envelopes (delta-driven when
    // the backend supports it), three-way diff against the snapshot,
    // then apply hunks via the worker pool. Each mailbox gets one
    // spinner that ends with `success` when hunks fired or `clear`
    // when the mailbox was already in sync; no-op accounts therefore
    // stay quiet.
    //
    // The LCD checkpoint refresh runs as a side-effect of
    // `fetch_side_envelopes`; new state bytes collect into
    // `new_states` and fold into the snapshot at stage 4b.
    let mut new_states: HashMap<(Side, String), Vec<u8>> = HashMap::new();
    let total_mailboxes = common.len();

    if total_mailboxes == 0 {
        let filter_kind = match &mailbox_filter {
            MailboxFilter::All => "all",
            MailboxFilter::Include(_) => "include",
            MailboxFilter::Exclude(_) => "exclude",
        };
        info!(
            "no common mailbox after filter ({filter_kind}): left has {left_count}, \
             right has {right_count}, intersection is empty",
            left_count = left_filtered.len(),
            right_count = right_filtered.len(),
        );
    }

    for (index, mailbox) in common.iter().enumerate() {
        let position = index + 1;
        let prefix = format!("[{position}/{total_mailboxes}] Syncing {mailbox}");
        let s = Spinner::start(format!("{prefix} (0%)"));
        info!("resolving messages in `{mailbox}` on both sides");

        // Treat a side that does not (yet) have the mailbox as having
        // an empty pair list. In real-mode the create ran in stage 2
        // so the listing would return empty anyway; in dry-run the
        // side never gained the mailbox so we skip the call entirely.
        // Either way the diff sees one populated side and emits
        // `Copy` hunks for every message on it.
        let left_pairs: EnvelopePairs = if left_filtered.contains(mailbox) {
            fetch_side_envelopes(
                &mut pool.left[0],
                Side::Left,
                mailbox,
                &snapshot,
                &mut new_states,
            )?
        } else {
            Vec::new()
        };
        let right_pairs: EnvelopePairs = if right_filtered.contains(mailbox) {
            fetch_side_envelopes(
                &mut pool.right[0],
                Side::Right,
                mailbox,
                &snapshot,
                &mut new_states,
            )?
        } else {
            Vec::new()
        };

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

        // Capture the post-fetch / pre-apply per-mailbox snapshot
        // inline. This is the baseline that the post-apply outcome
        // loop below mutates in place: each successful Copy inserts
        // the new target-side entry, Delete removes it, AddFlags /
        // RemoveFlags update the flag set. By the time the snapshot
        // persists at stage 5 it reflects the post-apply state, so
        // the next sync's diff sees the just-applied work as the
        // baseline and does no extra parsing.
        if !dry_run {
            snapshot.set_messages(Side::Left, mailbox.clone(), pairs_to_snapshot(&left_pairs));
            snapshot.set_messages(
                Side::Right,
                mailbox.clone(),
                pairs_to_snapshot(&right_pairs),
            );
        }

        let total = hunks.len();
        if total == 0 {
            s.clear();
            continue;
        }

        info!("applying {total} message hunks in `{mailbox}`");

        if dry_run {
            for hunk in hunks {
                report.email.patch.push(PatchEntry::new(hunk, None));
            }
        } else {
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

    // 4b. fold the freshly probed checkpoints into the in-memory
    // snapshot so cache::record at stage 5 persists them alongside
    // the post-sync envelope listing.
    for ((side, mailbox), state) in new_states.drain() {
        snapshot.set_state(side, mailbox, state);
    }
    for (side, state) in new_mailbox_states.drain() {
        snapshot.set_mailbox_state(side, state);
    }

    // 5. persist post-sync snapshot
    if !dry_run {
        let s = Spinner::start("Persisting snapshot…");
        info!(
            "persisting post-sync snapshot at `{}`",
            cache_path.display()
        );
        snapshot.record(&report.mailbox.patch, &cache_path)?;
        s.success("Persisted snapshot");
    }

    Ok(report)
}
