//! # Sync output
//!
//! The end-of-run summary the sync engine returns: `Display` for the
//! terminal, `Serialize` for `--json`, `JsonSchema` for `json-schema`.

use std::fmt;

use schemars::JsonSchema;
use serde::Serialize;

use crate::sync::hunk::{CollectionHunk, ItemHunk};

/// What `neverest sync` reports: the changes applied, plus what it left.
///
/// The exit code reads off it rather than off the process: a run that
/// reconciled its collections and still parked something is neither a
/// success nor a failure.
#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncOutput {
    /// The account the run reconciled.
    pub account: String,
    /// Whether the run only planned the patch instead of applying it.
    pub dry_run: bool,
    /// The collection-level patch, one entry per hunk.
    pub collection: PatchOutcome<CollectionHunk>,
    /// The item-level patch, one entry per hunk.
    pub item: PatchOutcome<ItemHunk>,
    /// Content-key collisions this sync surfaced; the first entry is kept.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collisions: Vec<MessageCollision>,
    /// Frontend-queued actions the pre-sync drain applied, per collection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drained: Vec<DrainedQueue>,
    /// Queue actions parked as permanently unappliable, surfaced until an
    /// operator repairs them (counted as warnings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parked: Vec<ParkedQueueAction>,
    /// The queued submit intents attempted this run, one entry each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub submitted: Vec<SubmitEntry>,
    /// The queued calendar intents attempted this run, one entry each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intents: Vec<IntentEntry>,
    /// What the retention sweep reclaimed, when one ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purged: Option<PurgedItems>,
    /// Items this run left conflicted (counted as warnings).
    ///
    /// Their content diverged on both sides beyond what the run's own
    /// three-way merge could settle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<ItemConflict>,
    /// Items waiting for a decision, and what the exit code answers.
    ///
    /// Not the length of [`conflicts`](Self::conflicts): the engine emits
    /// nothing for a placement it already parked, which is what keeps a
    /// five-minute schedule from notifying about one card all day.
    #[serde(default)]
    pub outstanding_conflicts: usize,
    /// Creates a side refused because it already holds the item's `UID`.
    ///
    /// Re-reported by every run until that side stops holding the identity
    /// twice (counted as warnings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refused: Vec<RefusedDuplicate>,
    /// Writes a remote refused, so the change stayed in the store.
    ///
    /// Re-tried and re-reported by every run until it lands or an operator
    /// removes the reason (counted as warnings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected: Vec<RejectedWrite>,
    /// The endpoints the run could not open: a server it could not reach, or
    /// one refusing the connection or its credentials.
    ///
    /// Each also reads as a failed `*` scan in the collection patch, the run
    /// exiting 3; this names it for a program. A collection that failed to
    /// read on an endpoint that opened never lands here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreached: Vec<UnreachedEndpoint>,
    /// The sources that gave up after their provider throttled them (HTTP
    /// 429, 503, a Google rate limit) past the waits neverest takes.
    ///
    /// What landed before stays; the run is incomplete (exit 3) and a rerun
    /// after `until` picks up the rest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub throttled: Vec<ThrottledSource>,
}

impl SyncOutput {
    /// Folds a collection-scoped report into this account-wide one.
    ///
    /// Every arm a collection can fill travels: merging back only the item
    /// patch loses the warning block the conflict notification fires from.
    /// The name, dry-run flag, sweep and outstanding count are account-wide.
    pub fn absorb(&mut self, other: Self) {
        // NOTE: every field is named, none elided, so a field added to the
        // report is a compile error here rather than an arm that silently
        // stops travelling: that is how the parked conflicts went missing.
        let Self {
            account: _,
            dry_run: _,
            collection,
            item,
            collisions,
            drained,
            parked,
            submitted,
            intents,
            purged: _,
            conflicts,
            outstanding_conflicts: _,
            refused,
            rejected,
            unreached,
            throttled,
        } = other;

        self.collection.patch.extend(collection.patch);
        self.item.patch.extend(item.patch);
        self.collisions.extend(collisions);
        self.drained.extend(drained);
        self.parked.extend(parked);
        self.submitted.extend(submitted);
        self.intents.extend(intents);
        self.conflicts.extend(conflicts);
        self.refused.extend(refused);
        self.rejected.extend(rejected);
        self.unreached.extend(unreached);
        self.throttled.extend(throttled);
    }

    /// Records a divergence this run parked, unless the run named it already.
    ///
    /// A collection is reconciled until quiescent, so several passes report
    /// into one account report. The engine says nothing about a placement it
    /// already parked, so a repeat is one divergence: one line, one warning.
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

    /// Whether the run left work no rerun clears, which the exit code answers.
    ///
    /// A divergence waiting for a decision, a duplicate `UID` the other side
    /// will not take and a write a remote refused are one class: re-reported
    /// every run until a person acts, and none of them a failure of the run.
    pub fn left_waiting(&self) -> bool {
        self.outstanding_conflicts > 0 || !self.refused.is_empty() || !self.rejected.is_empty()
    }

    /// Whether the run left work a rerun picks up: a source it could not
    /// scan, a hunk it could not apply, a send or an intent that failed
    /// without parking, a source that gave up throttled. An unreachable
    /// server reads this way, and only this way, a run with nothing to do
    /// never carrying an error.
    pub fn incomplete(&self) -> bool {
        !self.throttled.is_empty()
            || self
                .collection
                .patch
                .iter()
                .any(|entry| entry.error.is_some())
            || self.item.patch.iter().any(|entry| entry.error.is_some())
            || self
                .submitted
                .iter()
                .any(|entry| entry.error.is_some() && !entry.parked)
            || self
                .intents
                .iter()
                .any(|entry| entry.error.is_some() && !entry.parked)
    }
}

/// A source that gave up after its provider throttled it.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThrottledSource {
    /// The source, as the account names it.
    pub source: String,
    /// Until when it waits, RFC 3339 UTC: the `Retry-After` the provider
    /// stated, or the next back-off wait.
    pub until: String,
}

/// An endpoint whose connections the run could not open.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnreachedEndpoint {
    /// The source or target, as the account names it.
    pub endpoint: String,
    /// Why, the whole error chain.
    pub error: String,
}

/// One create refused for `no-uid-conflict` (RFC 4791 §5.3.2, RFC 6352 §6.3.2).
///
/// Not about the duplicate, which the store mirrors as two items, but about
/// the write that could not land, retried and re-reported every run. Giving
/// one copy a `UID` of its own is the user's call, made in their own client.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefusedDuplicate {
    /// The side that refused the write.
    pub side: String,
    /// The collection the copy was headed for.
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

/// One write a remote would not take, or that never reached the wire.
///
/// Carried here rather than among the applied hunks, so `already in sync`
/// keeps meaning the run wrote nothing. The store still holds the change and
/// the next run retries; a refusal repeated forever is one a person acts on.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RejectedWrite {
    /// The side that refused the write.
    pub side: String,
    /// The collection the item sits in.
    pub collection: String,
    /// The item's handle on that side.
    pub id: String,
    /// What the run tried: `update`, `append`, `delete`, `move`, `set flags`.
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

/// One item both sides changed against a shared base, left conflicted.
///
/// The residue of the three-way merge, which already took both sides where
/// they touched different fields; whose edit wins is itself an edit, staged
/// through the pimdir queue. Only mutable-content kinds reach this state.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemConflict {
    /// The side the divergence is against.
    pub side: String,
    /// The collection the item sits in.
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
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DrainedQueue {
    /// The collection the drained actions were anchored on.
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

/// One queue action the drain parked as permanently unappliable.
///
/// It stays queryable in the store until repaired, and every run reports it
/// again.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParkedQueueAction {
    /// The queue row's global append id.
    pub id: i64,
    /// The collection the action was anchored on.
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

/// One submit intent attempted this run.
///
/// Acknowledged when `error` is `None`, its queue row dropped and the body's
/// pin released; parked when the failure is permanent, pending otherwise.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmitEntry {
    /// The queue row's global append id.
    pub id: i64,
    /// The collection the intent was anchored on.
    pub collection: String,
    /// The submitted item's subject, when the intent payload carried one.
    pub subject: Option<String>,
    /// Formatted send error; `None` on success.
    pub error: Option<String>,
    /// Whether the failure parked the row rather than leaving it pending.
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

/// One calendar intent attempted this run (pimdir STORAGE Annex B.2).
///
/// Acknowledged when `error` is `None`, its queue row dropped; parked when
/// the failure is permanent, pending otherwise. The effect itself, the new
/// `PARTSTAT` or the removal, arrives with the next sync.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntentEntry {
    /// The queue row's global append id.
    pub id: i64,
    /// The collection of the item it addresses.
    pub collection: String,
    /// The intent kind: `calendar-reply` or `calendar-cancel`.
    pub kind: String,
    /// The source that performed it.
    pub source: String,
    /// The public id of the item, when the payload named one.
    pub seq: Option<i64>,
    /// Formatted provider error; `None` on success.
    pub error: Option<String>,
    /// Whether the failure parked the row rather than leaving it pending.
    pub parked: bool,
}

impl fmt::Display for IntentEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            id, kind, source, ..
        } = self;
        match (&self.error, self.parked) {
            (None, _) => write!(f, "{kind} #{id} performed by {source}"),
            (Some(err), true) => write!(f, "{kind} #{id} parked, never retried: {err}"),
            (Some(err), false) => write!(f, "{kind} #{id} not performed, retried next run: {err}"),
        }
    }
}

/// What the retention sweep reclaimed past `store.purge-after`.
///
/// Two operations, because a purge releases a body without reclaiming one:
/// the row goes with its reference, and the bytes are the collector's to take
/// once nothing else points at them, so `bytes` is what this run freed.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
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
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MessageCollision {
    /// The side the colliding envelopes were read from.
    pub side: String,
    /// The collection they sit in.
    pub collection: String,
    /// Shared `Message-ID:` when every envelope carried one.
    ///
    /// `None` when the legacy `(subject, date, from)` fallback collapsed
    /// envelopes without a header.
    pub message_id: Option<String>,
    /// The colliding handles, the kept one first.
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

/// One level of the patch a run applied, collection or item.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchOutcome<H> {
    /// The hunks the run applied at that level, in the order it applied
    /// them.
    pub patch: Vec<PatchEntry<H>>,
}

impl<H> Default for PatchOutcome<H> {
    fn default() -> Self {
        Self { patch: Vec::new() }
    }
}

/// One hunk of a patch, and whether applying it worked.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchEntry<H> {
    /// The change the run tried to make.
    pub hunk: H,
    /// Formatted apply error (`{e:#}`); `None` on success.
    pub error: Option<String>,
}

impl<H> PatchEntry<H> {
    /// Records one hunk and, where the apply failed, how it read.
    pub fn new(hunk: H, error: Option<anyhow::Error>) -> Self {
        Self {
            hunk,
            error: error.map(|e| format!("{e:#}")),
        }
    }
}

impl fmt::Display for SyncOutput {
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
        let intent_errors = self.intents.iter().filter(|e| e.error.is_some()).count();
        let errors = mailbox_errors + item_errors + submit_errors + intent_errors;
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

        if !self.intents.is_empty() {
            writeln!(f, "Calendar intents ({n}):", n = self.intents.len())?;
            for entry in &self.intents {
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
