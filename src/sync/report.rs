//! End-of-run summary returned by the sync engine; implements `Display`
//! for the terminal and `Serialize` for `--json`.

use std::fmt;

use serde::Serialize;

use crate::sync::hunk::{CollectionHunk, ItemHunk};

#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    pub account: String,
    pub dry_run: bool,
    /// What the store keeps, per namespace. Always reported, including on a
    /// run that wrote nothing: it is derived rather than configured, so the
    /// report is the only place it is ever stated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub namespaces: Vec<NamespaceReport>,
    pub collection: PatchOutcome<CollectionHunk>,
    pub item: PatchOutcome<ItemHunk>,
    /// Content-key collisions surfaced this sync (first envelope kept,
    /// rest skipped).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collisions: Vec<MessageCollision>,
    /// Frontend-queued actions the pre-sync drain applied, per
    /// collection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drained: Vec<DrainedQueue>,
    /// Queue actions parked as permanently unappliable, surfaced until
    /// an operator repairs them (counted as warnings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parked: Vec<ParkedQueueAction>,
    /// The queued submit intents attempted this run, one entry each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submitted: Vec<SubmitEntry>,
    /// What the retention sweep reclaimed, when one ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purged: Option<PurgedItems>,
    /// Items whose content diverged on both sides and were left conflicted.
    /// Re-reported by every run until resolved (counted as warnings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<ItemConflict>,
    /// Identities a side holds under more than one handle, which the engine
    /// froze. Re-reported by every run until the collection holds each once
    /// (counted as warnings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ambiguous: Vec<AmbiguousIdentity>,
}

/// What one hub namespace holds, and what the store keeps for it.
///
/// Reported on every run because nothing else states it: `store.retention` and
/// `store.hydration` are gone, and the value is derived from how many sources
/// share the namespace. Someone who set up a two-source backup expecting the
/// store to *be* the backup learns it here, on run one, rather than on the day
/// they need it.
#[derive(Debug, Serialize)]
pub struct NamespaceReport {
    /// The kind every source in the namespace syncs.
    pub media_type: String,
    pub namespace: String,
    /// The namespace's source names, sorted.
    pub sources: Vec<String>,
    /// What the store keeps: `none`, `crossing` or `all`.
    pub bodies: String,
    /// The value the previous run derived, when it differed. A namespace that
    /// gained a source flips from keeping every body to keeping none, so the
    /// change is named rather than left to be noticed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<String>,
}

impl fmt::Display for NamespaceReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            media_type,
            namespace,
            sources,
            bodies,
            was,
        } = self;

        let list = sources.join(", ");
        write!(f, "{media_type} / {namespace} ({list}): bodies {bodies}")?;

        if let Some(was) = was {
            write!(
                f,
                " (was {was}; bodies already stored are kept, unreferenced, until `pimdir gc`)"
            )?;
        }

        Ok(())
    }
}

/// One identity a side's collection holds more than once: two items carrying
/// the same link id, two messages with one `Message-ID`, so which copy a
/// change belongs to cannot be decided.
///
/// Neverest reports the coordinates and repairs nothing. Which copy to keep is
/// the user's call, made with their own client, and the report says what
/// neverest cannot tell apart rather than that the collection is invalid: RFC
/// 5322 §3.6.4 binds the *generator* of a `Message-ID` and says nothing about
/// what a store may hold, a copy legitimately carries the identifier of the
/// message it copies, and a migration (this tool's own use case) commonly
/// produces such a pair.
///
/// Detection, policy and state belong to the engine and the store, which
/// persist the freeze, so this comes back on every run until the collection
/// holds the identity once. A warning the user cannot act on twice is a
/// warning they will not act on once.
#[derive(Debug, Serialize)]
pub struct AmbiguousIdentity {
    pub side: String,
    pub collection: String,
    /// Every handle the side holds the identity under, the bound one first.
    pub ids: Vec<String>,
}

impl fmt::Display for AmbiguousIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            collection,
            ids,
        } = self;
        let count = ids.len();
        let list = ids
            .iter()
            .map(|id| format!("`{id}`"))
            .collect::<Vec<_>>()
            .join(", ");
        write!(
            f,
            "{count} copies of one item in `{collection}` on {side} ({list}): neverest cannot tell them apart, so none of them syncs until one is removed"
        )
    }
}

/// One item left conflicted: its content changed on both sides against a
/// shared base, so neither could win without losing an edit.
///
/// Neverest never resolves one by itself — resolution is an edit, and whose
/// edit wins is not a decision a sync run can make. The item is reported here
/// on every run until someone settles it (a frontend stages the merged body
/// through the pimdir queue's `update` action).
///
/// Only mutable-content kinds can reach this state: mail bodies are immutable,
/// so a mail sync never reports a conflict.
#[derive(Debug, Serialize)]
pub struct ItemConflict {
    pub side: String,
    pub collection: String,
    /// The item's handle on that side.
    pub id: String,
}

impl fmt::Display for ItemConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            collection,
            id,
        } = self;
        write!(
            f,
            "item `{id}` in `{collection}` on {side} changed on both sides and is left conflicted"
        )
    }
}

/// One collection's applied count from the pre-sync queue drain.
#[derive(Debug, Serialize)]
pub struct DrainedQueue {
    pub collection: String,
    /// Actions applied to the store and deleted from the queue.
    pub applied: usize,
}

impl fmt::Display for DrainedQueue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            collection,
            applied,
        } = self;
        write!(f, "applied {applied} queued action(s) in `{collection}`")
    }
}

/// One queue action the drain parked as permanently unappliable
/// (malformed payload, unknown target); it stays queryable in the store
/// until repaired and is re-reported by every run.
#[derive(Debug, Serialize)]
pub struct ParkedQueueAction {
    /// The queue row's global append id.
    pub id: i64,
    pub collection: String,
    /// The raw action kind (`add`, `set_flags`, …).
    pub action: String,
    /// The enqueuing process, diagnostic only.
    pub producer: String,
    /// The failure that parked the row.
    pub error: String,
}

impl fmt::Display for ParkedQueueAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            id,
            collection,
            action,
            producer,
            error,
        } = self;
        write!(
            f,
            "parked queue action #{id} (`{action}` in `{collection}` from `{producer}`): {error}"
        )
    }
}

/// One submit intent attempted this run: acknowledged (its queue row
/// dropped, releasing the body's pin) when `error` is `None`, parked when
/// the failure is permanent, left pending otherwise.
#[derive(Debug, Serialize)]
pub struct SubmitEntry {
    /// The queue row's global append id.
    pub id: i64,
    /// The collection the intent was anchored on.
    pub collection: String,
    /// The submitted item's subject, when the intent payload carried one.
    pub subject: Option<String>,
    /// Formatted send error; `None` on success.
    pub error: Option<String>,
    /// Whether the failure parked the row (permanent) rather than leaving
    /// it pending for the next run (transient). Always false on success.
    pub parked: bool,
}

impl fmt::Display for SubmitEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let subject = self.subject.as_deref().unwrap_or("<no subject>");
        match (&self.error, self.parked) {
            (None, _) => write!(f, "submitted `{subject}`"),
            (Some(err), true) => write!(f, "`{subject}` parked, never retried: {err}"),
            (Some(err), false) => write!(f, "`{subject}` not submitted, retried next run: {err}"),
        }
    }
}

/// What the retention sweep reclaimed: the retained (soft-deleted) items
/// purged past `store.purge-after`, and the bytes the collector then freed.
///
/// The two are counted by two operations, because a purge releases a body and
/// does not reclaim one: the row goes and its reference with it, and the bytes
/// are the collector's to take once nothing else points at them. A body a live
/// item still holds survives both, so `bytes` is what this run actually freed
/// rather than what the purged items were holding.
#[derive(Debug, Serialize)]
pub struct PurgedItems {
    /// Retained items deleted for good.
    pub items: usize,
    /// Object rows the collector dropped afterwards.
    pub objects: usize,
    /// Blob bytes it freed with them.
    pub bytes: u64,
}

impl fmt::Display for PurgedItems {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            items,
            objects,
            bytes,
        } = self;
        write!(
            f,
            "purged {items} retained item(s), collected {objects} object(s), {bytes} byte(s) reclaimed"
        )
    }
}

/// One content-key collision group; first id in `ids` is the kept one.
#[derive(Debug, Serialize)]
pub struct MessageCollision {
    pub side: String,
    pub collection: String,
    /// Shared `Message-ID:` when every envelope carried one; `None`
    /// when the legacy `(subject, date, from)` fallback collapsed
    /// envelopes without a header.
    pub message_id: Option<String>,
    pub ids: Vec<String>,
}

impl fmt::Display for MessageCollision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            collection,
            message_id,
            ids,
        } = self;
        let kept = ids.first().map(String::as_str).unwrap_or("?");
        let skipped = ids
            .iter()
            .skip(1)
            .map(|id| format!("`{id}`"))
            .collect::<Vec<_>>()
            .join(", ");
        match message_id {
            Some(mid) => write!(
                f,
                "skip {skipped} on {side} `{collection}`: same Message-ID `{mid}` as `{kept}`"
            ),
            None => write!(
                f,
                "skip {skipped} on {side} `{collection}`: same subject/date/sender as `{kept}` (no Message-ID header)"
            ),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PatchOutcome<H> {
    pub patch: Vec<PatchEntry<H>>,
}

impl<H> Default for PatchOutcome<H> {
    fn default() -> Self {
        Self { patch: Vec::new() }
    }
}

#[derive(Debug, Serialize)]
pub struct PatchEntry<H> {
    pub hunk: H,
    /// Formatted apply error (`{e:#}`); `None` on success.
    pub error: Option<String>,
}

impl<H> PatchEntry<H> {
    pub fn new(hunk: H, error: Option<anyhow::Error>) -> Self {
        Self {
            hunk,
            error: error.map(|e| format!("{e:#}")),
        }
    }
}

impl fmt::Display for SyncReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f)?;

        let total = self.collection.patch.len() + self.item.patch.len();
        let mailbox_errors = self
            .collection
            .patch
            .iter()
            .filter(|e| e.error.is_some())
            .count();
        let item_errors = self.item.patch.iter().filter(|e| e.error.is_some()).count();
        let submit_errors = self.submitted.iter().filter(|e| e.error.is_some()).count();
        let errors = mailbox_errors + item_errors + submit_errors;
        let warnings =
            self.collisions.len() + self.parked.len() + self.conflicts.len() + self.ambiguous.len();

        if !self.namespaces.is_empty() {
            writeln!(f, "Store ({n}):", n = self.namespaces.len())?;
            for entry in &self.namespaces {
                writeln!(f, " - {entry}")?;
            }
            writeln!(f)?;
        }

        if !self.drained.is_empty() {
            writeln!(f, "Queue ({n}):", n = self.drained.len())?;
            for entry in &self.drained {
                writeln!(f, " - {entry}")?;
            }
            writeln!(f)?;
        }

        if !self.submitted.is_empty() {
            writeln!(f, "Submissions ({n}):", n = self.submitted.len())?;
            for entry in &self.submitted {
                writeln!(f, " - {entry}")?;
            }
            writeln!(f)?;
        }

        if !self.collection.patch.is_empty() {
            writeln!(
                f,
                "Collection patches ({n}):",
                n = self.collection.patch.len()
            )?;
            for entry in &self.collection.patch {
                writeln!(f, " - {hunk}", hunk = entry.hunk)?;
            }
            writeln!(f)?;
        }

        if !self.item.patch.is_empty() {
            writeln!(f, "Item patches ({n}):", n = self.item.patch.len())?;
            for entry in &self.item.patch {
                writeln!(f, " - {hunk}", hunk = entry.hunk)?;
            }
            writeln!(f)?;
        }

        if let Some(purged) = &self.purged
            && purged.items > 0
        {
            writeln!(f, "Retention:")?;
            writeln!(f, " - {purged}")?;
            writeln!(f)?;
        }

        if warnings > 0 {
            writeln!(f, "Warnings ({warnings}):")?;
            for c in &self.collisions {
                writeln!(f, " - {c}")?;
            }
            for c in &self.conflicts {
                writeln!(f, " - {c}")?;
            }
            for a in &self.ambiguous {
                writeln!(f, " - {a}")?;
            }
            for p in &self.parked {
                writeln!(f, " - {p}")?;
            }
            writeln!(f)?;
        }

        if errors > 0 {
            writeln!(f, "Errors ({errors}):")?;
            for entry in self.collection.patch.iter().filter(|e| e.error.is_some()) {
                writeln!(
                    f,
                    " - {hunk}: {err}",
                    hunk = entry.hunk,
                    err = entry.error.as_deref().unwrap_or_default(),
                )?;
            }
            for entry in self.item.patch.iter().filter(|e| e.error.is_some()) {
                writeln!(
                    f,
                    " - {hunk}: {err}",
                    hunk = entry.hunk,
                    err = entry.error.as_deref().unwrap_or_default(),
                )?;
            }
            writeln!(f)?;
        }

        let account = &self.account;
        match (total, errors, warnings, self.dry_run) {
            (0, 0, 0, _) => writeln!(f, "Account `{account}` is already in sync"),
            (0, 0, w, _) => writeln!(f, "Account `{account}` is already in sync ({w} warnings)"),
            (n, 0, 0, true) => writeln!(f, "Account `{account}` would apply {n} hunks"),
            (n, 0, w, true) => writeln!(
                f,
                "Account `{account}` would apply {n} hunks ({w} warnings)"
            ),
            (n, e, 0, true) => writeln!(
                f,
                "Account `{account}` would apply {n} hunks ({e} would fail)"
            ),
            (n, e, w, true) => writeln!(
                f,
                "Account `{account}` would apply {n} hunks ({e} would fail, {w} warnings)"
            ),
            (n, 0, 0, false) => writeln!(f, "Account `{account}` synchronized: {n} hunks"),
            (n, 0, w, false) => writeln!(
                f,
                "Account `{account}` synchronized: {n} hunks, {w} warnings"
            ),
            (n, e, 0, false) => writeln!(
                f,
                "Account `{account}` partially synchronized: {n} hunks, {e} errors"
            ),
            (n, e, w, false) => writeln!(
                f,
                "Account `{account}` partially synchronized: {n} hunks, {e} errors, {w} warnings"
            ),
        }
    }
}
