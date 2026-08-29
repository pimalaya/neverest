//! End-of-run summary returned by the sync engine; implements `Display`
//! for the terminal and `Serialize` for `--json`.

use std::fmt;

use serde::Serialize;

use crate::sync::hunk::{CollectionHunk, ItemHunk};

#[derive(Debug, Default, Serialize)]
pub struct SyncReport {
    pub account: String,
    pub dry_run: bool,
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
    /// Items whose content diverged on both sides beyond what the run's own
    /// three-way merge could settle, and which this run therefore left
    /// conflicted (counted as warnings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<ItemConflict>,
    /// How many items the store holds waiting for a decision, whichever run
    /// marked them. This is what the run's exit code answers.
    ///
    /// Not the length of [`conflicts`](Self::conflicts), and the difference
    /// matters: the engine emits nothing for a placement it already parked,
    /// which is what keeps a five-minute schedule from notifying about one
    /// card three hundred times a day, and which is also why the run's own
    /// tally is not the number of decisions waiting.
    #[serde(default)]
    pub outstanding_conflicts: usize,
    /// Creates a side refused because it already holds the item's `UID`.
    /// Re-reported by every run until that side stops holding the identity
    /// twice (counted as warnings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refused: Vec<RefusedDuplicate>,
    /// Writes a remote refused, so the change stayed in the store. Re-tried
    /// and re-reported by every run until it lands or an operator removes
    /// the reason (counted as warnings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected: Vec<RejectedWrite>,
}

impl SyncReport {
    /// Folds a collection-scoped report into this account-wide one.
    ///
    /// Every arm a collection can fill travels, and there is exactly one
    /// place that decides which: a run that reconciled a collection in a
    /// worker thread and merged back only its item patch would say how many
    /// items it touched and never what it parked, which is the whole of the
    /// warning block and the trigger the conflict notification fires from.
    ///
    /// What does not travel is what a collection never had an opinion about:
    /// the account's own name and dry-run flag, the retention sweep, which
    /// runs once for the account, and the outstanding conflict count, which
    /// is read from the store once rather than summed over collections.
    pub fn absorb(&mut self, other: Self) {
        // NOTE: every field is named, none elided, so a field added to the
        // report is a compile error here rather than an arm that silently
        // stops travelling, which is how the parked conflicts went missing.
        let Self {
            account: _,
            dry_run: _,
            collection,
            item,
            collisions,
            drained,
            parked,
            submitted,
            purged: _,
            conflicts,
            outstanding_conflicts: _,
            refused,
            rejected,
        } = other;

        self.collection.patch.extend(collection.patch);
        self.item.patch.extend(item.patch);
        self.collisions.extend(collisions);
        self.drained.extend(drained);
        self.parked.extend(parked);
        self.submitted.extend(submitted);
        self.conflicts.extend(conflicts);
        self.refused.extend(refused);
        self.rejected.extend(rejected);
    }

    /// Records a divergence this run parked, unless the run named it already.
    ///
    /// A collection is reconciled until it is quiescent, so a pass runs
    /// several times over one collection and both endpoints report into one
    /// account report. The engine says nothing about a placement it has
    /// already parked, so a repeat means the run's own merge settled the
    /// divergence and a later pass marked it again: one divergence, one line,
    /// and one notification. Which is also why the number of decisions
    /// waiting is read from the store instead of counted here.
    pub fn note_conflict(&mut self, conflict: ItemConflict) {
        let named = self.conflicts.iter().any(|named| {
            named.side == conflict.side
                && named.collection == conflict.collection
                && named.id == conflict.id
        });

        if !named {
            self.conflicts.push(conflict);
        }
    }

    /// Whether the run left work behind that no rerun clears on its own,
    /// which is what the exit code answers.
    ///
    /// Three states qualify, and they are one class: a divergence waiting for
    /// a decision, a duplicate `UID` the other side will not take, and a
    /// write the remote refused. Each leaves the store holding something it
    /// could not deliver, each is re-reported on every run until a person
    /// acts, and none of them is a failure of the run.
    pub fn left_waiting(&self) -> bool {
        self.outstanding_conflicts > 0 || !self.refused.is_empty() || !self.rejected.is_empty()
    }
}

/// One create a side refused with the CalDAV or CardDAV `no-uid-conflict`
/// precondition (RFC 4791 §5.3.2, RFC 6352 §6.3.2): the collection the copy
/// was going into already holds a resource carrying that `UID`.
///
/// A collection holding one identity twice is mirrored as two items and says
/// nothing (the store holds what a source holds), so this is not about the
/// duplicate. It is about the write that could not land: the other side will
/// not take the second copy, so the run wrote nothing for it and will try
/// again next run.
///
/// The repetition is the point. It is an unresolved state with an action
/// attached, namely giving one of the two copies a `UID` of its own on the
/// side that holds them both, and neverest performs none of it: which copy is
/// which is the user's call, made with their own client.
#[derive(Debug, Serialize)]
pub struct RefusedDuplicate {
    pub side: String,
    pub collection: String,
    /// The identity the refused copy shares with the resource already there.
    pub uid: String,
}

impl fmt::Display for RefusedDuplicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            collection,
            uid,
        } = self;
        write!(
            f,
            "{side} refused a copy in {collection}: it already holds UID {uid}, so the second copy stays unwritten until one of the two carries a UID of its own"
        )
    }
}

/// One write a remote would not take: a body it rejected, a flag change it
/// refused, a delete it would not perform.
///
/// A hunk is the plan the run derived, and until the remote answers, a plan
/// is all it is. A write that did not land is therefore not one of the hunks
/// the run reports having applied: it is carried here instead, so
/// `already in sync` keeps meaning the run wrote nothing and the hunk count
/// keeps meaning what reached a server.
///
/// The repetition is the point, as it is for [`RefusedDuplicate`]. The store
/// still holds the change, the next run tries again, and a refusal a server
/// repeats forever (a body it will not parse, a collection it made read-only)
/// is one a person has to act on. A refusal on the way to the wire, a body
/// the blob tree lost, is named the same way for the same reason.
#[derive(Debug, Serialize)]
pub struct RejectedWrite {
    pub side: String,
    pub collection: String,
    /// The item's handle on that side.
    pub id: String,
    /// What the run was trying to do: `update`, `append`, `delete`, `move`
    /// or `set flags`.
    pub action: String,
    /// Why it did not land, as the backend put it.
    pub reason: String,
}

impl fmt::Display for RejectedWrite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            collection,
            id,
            action,
            reason,
        } = self;
        write!(
            f,
            "{side} refused the {action} of {id} in {collection}, so it stays in the store: {reason}"
        )
    }
}

/// One item left conflicted: both sides changed the same field against a
/// shared base, so neither could win without losing an edit.
///
/// This is the residue of the run's own three-way merge, which takes both
/// sides wherever they touched different fields. What is left is a genuine
/// disagreement, and whose edit wins is not a decision a sync run can make:
/// resolution is an edit, staged through the pimdir queue's `update` action
/// by whoever owns it.
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
            "item {id} in {collection} on {side} changed on both sides and is left conflicted"
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
        write!(f, "applied {applied} queued action(s) in {collection}")
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
            "parked queue action #{id} ({action} in {collection} from {producer}): {error}"
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
            (None, _) => write!(f, "submitted {subject}"),
            (Some(err), true) => write!(f, "{subject} parked, never retried: {err}"),
            (Some(err), false) => write!(f, "{subject} not submitted, retried next run: {err}"),
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
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        match message_id {
            Some(mid) => write!(
                f,
                "skip {skipped} on {side} {collection}: same Message-ID {mid} as {kept}"
            ),
            None => write!(
                f,
                "skip {skipped} on {side} {collection}: same subject/date/sender as {kept} (no Message-ID header)"
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
        let warnings = self.collisions.len()
            + self.parked.len()
            + self.conflicts.len()
            + self.refused.len()
            + self.rejected.len();

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
            for r in &self.refused {
                writeln!(f, " - {r}")?;
            }
            for r in &self.rejected {
                writeln!(f, " - {r}")?;
            }
            for p in &self.parked {
                writeln!(f, " - {p}")?;
            }
            writeln!(f)?;
        }

        if self.outstanding_conflicts > 0 {
            writeln!(
                f,
                "Conflicts: {n} item(s) waiting for a decision",
                n = self.outstanding_conflicts,
            )?;
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
            (0, 0, 0, _) => writeln!(f, "Account {account} is already in sync"),
            (0, 0, w, _) => writeln!(f, "Account {account} is already in sync ({w} warnings)"),
            (n, 0, 0, true) => writeln!(f, "Account {account} would apply {n} hunks"),
            (n, 0, w, true) => {
                writeln!(f, "Account {account} would apply {n} hunks ({w} warnings)")
            }
            (n, e, 0, true) => writeln!(
                f,
                "Account {account} would apply {n} hunks ({e} would fail)"
            ),
            (n, e, w, true) => writeln!(
                f,
                "Account {account} would apply {n} hunks ({e} would fail, {w} warnings)"
            ),
            (n, 0, 0, false) => writeln!(f, "Account {account} synchronized: {n} hunks"),
            (n, 0, w, false) => {
                writeln!(f, "Account {account} synchronized: {n} hunks, {w} warnings")
            }
            (n, e, 0, false) => writeln!(
                f,
                "Account {account} partially synchronized: {n} hunks, {e} errors"
            ),
            (n, e, w, false) => writeln!(
                f,
                "Account {account} partially synchronized: {n} hunks, {e} errors, {w} warnings"
            ),
        }
    }
}
