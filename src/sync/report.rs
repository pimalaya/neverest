//! # Sync output
//!
//! The end-of-run summary the sync engine returns: `Display` for the
//! terminal, `Serialize` for `--json`, `JsonSchema` for `json-schema`.

use std::{fmt, path::PathBuf};

use io_pimdir::{
    client::producer::PimdirParkedAction,
    codec::{PimdirAction, PimdirActionHandle, PimdirStoreIdentity, action_from_payload},
    intent::{PimdirIntentItem, PimdirInvitation},
};
use pimalaya_cli::error::ItemError;
use schemars::JsonSchema;
use serde::Serialize;

use crate::{
    cli::queue::QueueOutput,
    error::{ErrorCode, Failure, chain},
    sync::hunk::{CollectionHunk, ItemHunk},
};

/// What `neverest sync` reports: the outcome of every account the run
/// covered, in the order it took them.
#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncRunOutput {
    /// The accounts `--init` initialized before the run, empty without it.
    pub initialized: Vec<String>,
    /// One entry per account the run covered, empty under `--queue-only`
    /// and `--index-only`.
    pub accounts: Vec<AccountSync>,
    /// What `--queue-only` applied, one entry per account, empty otherwise.
    pub queue: Vec<QueueOutput>,
    /// What `--index-only` built, one entry per store, empty otherwise.
    pub index: Vec<IndexOutput>,
}

impl fmt::Display for SyncRunOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for account in &self.initialized {
            writeln!(f, "Initialized account {account}")?;
        }
        for account in &self.accounts {
            write!(f, "{account}")?;
        }
        for account in &self.queue {
            write!(f, "{account}")?;
        }
        for store in &self.index {
            write!(f, "{store}")?;
        }
        Ok(())
    }
}

/// One account's outcome in a sync run: what it reported, or why it did
/// not run.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountSync {
    /// The account.
    pub account: String,
    /// Whether another run held the account's store past the bounded wait,
    /// so the account did not run.
    pub busy: bool,
    /// The stable code of the failure that stopped the account, `null` when
    /// it ran or the failure carries none.
    pub code: Option<ErrorCode>,
    /// Why the account stopped short of a report, the outermost message,
    /// `null` when it ran.
    pub error: Option<String>,
    /// The messages of the failure's causes, outermost first, empty when it
    /// ran.
    pub sources: Vec<String>,
    /// What the run did and left on the account, `null` when it did not
    /// run.
    pub report: Option<SyncOutput>,
}

impl fmt::Display for AccountSync {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let account = &self.account;
        match (&self.report, &self.error) {
            (Some(report), _) => write!(f, "{report}"),
            (None, Some(error)) => {
                let error = chain(error, &self.sources);
                writeln!(f, "\nAccount {account} failed: {error}")
            }
            (None, None) => writeln!(
                f,
                "\nAccount {account} did not run: another run holds its store"
            ),
        }
    }
}

/// What `neverest sync` reports of one account: the changes applied, plus
/// what it left.
///
/// The exit code reads off it rather than off the process: a run that
/// reconciled its collections and still parked something is neither a
/// success nor a failure.
#[derive(Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncOutput {
    /// The account the run reconciled, which the account row around the
    /// report already names.
    #[serde(skip)]
    pub account: String,
    /// Whether the run only planned the patch instead of applying it.
    pub dry_run: bool,
    /// The collection-level hunks the run applied, in the order it applied
    /// them; one that failed is in [`errors`](Self::errors) instead.
    pub collection_patch: Vec<CollectionHunk>,
    /// The item-level hunks the run applied, in the order it applied them;
    /// one that failed is in [`errors`](Self::errors) instead.
    pub item_patch: Vec<ItemHunk>,
    /// Content-key collisions this sync surfaced; the first entry is kept.
    pub collisions: Vec<MessageCollision>,
    /// Frontend-queued actions the pre-sync drain applied, per collection.
    pub drained: Vec<DrainedQueue>,
    /// Queue rows the store holds parked, as `pimdir queue list` prints a
    /// row, until a person retries or cancels them (warnings, exit 3).
    pub parked: Vec<ParkedQueueAction>,
    /// The queued submit intents this run sent, one entry each; one that
    /// failed is in [`errors`](Self::errors) instead.
    pub submitted: Vec<SubmitEntry>,
    /// The queued calendar and collection intents this run performed, one
    /// entry each; one that failed is in [`errors`](Self::errors) instead.
    pub intents: Vec<IntentEntry>,
    /// What the retention sweep reclaimed, when one ran.
    pub purged: Option<PurgedItems>,
    /// Items this run left conflicted (counted as warnings).
    ///
    /// Their content diverged on both sides beyond what the run's own
    /// three-way merge could settle.
    pub conflicts: Vec<ItemConflict>,
    /// Items waiting for a decision, and what the exit code answers.
    ///
    /// Not the length of [`conflicts`](Self::conflicts): the engine emits
    /// nothing for a placement it already parked, which is what keeps a
    /// five-minute schedule from notifying about one card all day.
    pub outstanding_conflicts: usize,
    /// The sources that gave up after their provider throttled them (HTTP
    /// 429, 503, a Google rate limit) past the waits neverest takes.
    ///
    /// What landed before stays; the run is incomplete (exit 4) and a rerun
    /// after `until` picks up the rest.
    pub throttled: Vec<ThrottledSource>,
    /// What each source's collections are known to hold once the run ends
    /// (pimdir SYNC §5): the last closed round's scope and time, and any open.
    ///
    /// A round left open was interrupted (a failure, a throttled provider, a
    /// stopped run): the run is incomplete (exit 4), the next one resumes it.
    pub coverage: Vec<CollectionCoverage>,
    /// The octets each source received this run: the bodies and the meta
    /// of its listings, not the protocol around them.
    pub downloaded: Vec<SourceDownload>,
    /// The collections whose last convergence pass still moved something:
    /// the run is incomplete (exit 4) and a rerun goes on.
    pub unsettled: Vec<String>,
    /// Whether a termination signal stopped the run before it reconciled
    /// everything: it is incomplete (exit 4) and a rerun goes on.
    pub interrupted: bool,
    /// The listings that found most of a collection gone, or no member where
    /// it held some, and so dropped nothing (pimdir SYNC §5, §8; warnings).
    ///
    /// Read as truncated until confirmed with `sync --accept-absences`: a
    /// rerun parks again, so the run waits for a person (exit 3), its open
    /// round not counting as incomplete.
    pub absences: Vec<ParkedAbsence>,
    /// The changes withheld because the body they upload was missing from
    /// the store or did not match its name (pimdir STORAGE §7; warnings).
    ///
    /// A rerun makes no repair, so the run waits for a person (exit 3).
    pub unreadable: Vec<UnreadableBodies>,
    /// The bodies a policy did not adopt settling a side's write or applying
    /// a queued action, each kept as a retained item (pimdir SYNC §9).
    pub overruled: Vec<OverruledBody>,
    /// The pushes a side's server gave no outcome for, left pending: a
    /// rerun pushes them again, so the run is incomplete (exit 4).
    pub unanswered: Vec<UnansweredPushes>,
    /// The writes a source's configuration forbids, by collection and
    /// action: held in the store, never pushed, never counted as applied.
    ///
    /// No warning, no exit code: the configuration asked for it.
    pub forbidden: Vec<ForbiddenWrites>,
    /// The creates the last pass of each collection left waiting rather
    /// than pushing them, each with its reason (pimdir SYNC §5).
    ///
    /// No warning, no exit code: a later listing, a wider scope or its body
    /// lands each.
    pub waiting: Vec<WaitingCreate>,
    /// The members a listing named apart from a create of their side holding
    /// their identity, summaries disagreeing (pimdir SYNC §5, §6; warnings).
    ///
    /// Another message under the same `Message-ID`, or the create's own read
    /// otherwise by its server, which the item then holds twice.
    pub disagreed: Vec<DisagreedMember>,
    /// The members a capped source did not list in a closed round, counted
    /// per collection and kept (pimdir SYNC §5).
    ///
    /// No warning, no exit code: a capped source lists the same part at every
    /// round. Every narrowed round of neverest counts here too.
    pub unlisted: Vec<UnlistedMembers>,
    /// The open series a source read from a floor later than their start, an
    /// exception before it left out of the body (warnings).
    ///
    /// No exit code: the body is still synced, a rerun reads it the same way
    /// and no person can settle it.
    pub clipped: Vec<ClippedSeries>,
    /// What the run could not do: one item error each, `kind` naming what
    /// failed, then the keys that kind needs (pimalaya-cli's item error).
    ///
    /// A `rejected`, `duplicates` or `refusals` row and a parked send or
    /// intent wait for a person (exit 3); any other row waits for a rerun
    /// (exit 4), which wins.
    pub errors: Vec<SyncError>,
    /// What the run did to the store's search index (pimdir SEARCH), `null`
    /// in a dry run and in a build without the `search` feature.
    ///
    /// An index failure is reported here and counted as a warning, and
    /// never changes the exit code: the index is derived, the next run
    /// refreshes it again.
    pub index: Option<IndexRefresh>,
}

impl SyncOutput {
    /// Folds a collection-scoped report into this account-wide one.
    ///
    /// Every field is named, so a new one fails to compile here rather than
    /// silently stop travelling, as the parked conflicts once did. The name,
    /// dry-run flag, sweep and outstanding count are account-wide.
    pub fn absorb(&mut self, other: Self) {
        let Self {
            account: _,
            dry_run: _,
            collection_patch,
            item_patch,
            collisions,
            drained,
            parked,
            submitted,
            intents,
            purged: _,
            conflicts,
            outstanding_conflicts: _,
            throttled,
            coverage,
            downloaded,
            unsettled,
            absences,
            unreadable,
            overruled,
            unanswered,
            forbidden,
            waiting,
            disagreed,
            unlisted,
            clipped,
            interrupted,
            errors,
            index: _,
        } = other;

        self.collection_patch.extend(collection_patch);
        self.item_patch.extend(item_patch);
        self.collisions.extend(collisions);
        self.drained.extend(drained);
        self.parked.extend(parked);
        self.submitted.extend(submitted);
        self.intents.extend(intents);
        self.conflicts.extend(conflicts);
        self.throttled.extend(throttled);
        self.coverage.extend(coverage);
        self.downloaded.extend(downloaded);
        self.unsettled.extend(unsettled);
        self.absences.extend(absences);
        self.unreadable.extend(unreadable);
        self.overruled.extend(overruled);
        self.unanswered.extend(unanswered);
        self.forbidden.extend(forbidden);
        self.waiting.extend(waiting);
        self.disagreed.extend(disagreed);
        self.unlisted.extend(unlisted);
        self.clipped.extend(clipped);
        self.errors.extend(errors);
        self.interrupted |= interrupted;
    }

    /// Names every endpoint as the configuration does rather than by its
    /// store source id, `<account>/<name>`: the report names its account.
    pub fn name_endpoints(&mut self) {
        let prefix = format!("{}/", self.account);
        let mut endpoints: Vec<&mut String> = Vec::new();

        for hunk in &mut self.collection_patch {
            endpoints.extend(hunk.endpoints_mut());
        }
        for hunk in &mut self.item_patch {
            endpoints.extend(hunk.endpoints_mut());
        }
        for error in &mut self.errors {
            endpoints.extend(error.row.endpoints_mut());
        }
        endpoints.extend(self.collisions.iter_mut().map(|c| &mut c.side));
        endpoints.extend(self.forbidden.iter_mut().map(|f| &mut f.side));
        endpoints.extend(self.intents.iter_mut().map(|i| &mut i.source));
        endpoints.extend(self.conflicts.iter_mut().map(|c| &mut c.side));
        endpoints.extend(self.throttled.iter_mut().map(|t| &mut t.source));
        endpoints.extend(self.coverage.iter_mut().map(|c| &mut c.source));
        endpoints.extend(self.downloaded.iter_mut().map(|d| &mut d.source));
        endpoints.extend(self.absences.iter_mut().map(|a| &mut a.side));
        endpoints.extend(self.unreadable.iter_mut().map(|u| &mut u.side));
        endpoints.extend(self.overruled.iter_mut().filter_map(|o| o.side.as_mut()));
        endpoints.extend(self.unanswered.iter_mut().map(|u| &mut u.side));
        endpoints.extend(self.waiting.iter_mut().map(|w| &mut w.side));
        endpoints.extend(self.disagreed.iter_mut().map(|d| &mut d.side));
        endpoints.extend(self.unlisted.iter_mut().map(|u| &mut u.side));
        endpoints.extend(self.clipped.iter_mut().map(|c| &mut c.side));

        for endpoint in endpoints {
            if let Some(name) = endpoint.strip_prefix(&prefix) {
                *endpoint = name.to_owned();
            }
        }
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
                && named.remote_id == conflict.remote_id
        });

        if !named {
            self.conflicts.push(conflict);
        }
    }

    /// Records a collection hunk the run applied, or the failure that kept
    /// it from applying among the errors.
    pub fn note_collection_hunk(&mut self, hunk: CollectionHunk, error: Option<anyhow::Error>) {
        match error {
            None => self.collection_patch.push(hunk),
            Some(err) => self.note_error(None, &err, SyncErrorRow::CollectionPatch { hunk }),
        }
    }

    /// Records an item hunk the run applied, or the failure that kept it
    /// from applying among the errors.
    pub fn note_item_hunk(&mut self, hunk: ItemHunk, error: Option<anyhow::Error>) {
        match error {
            None => self.item_patch.push(hunk),
            Some(err) => self.note_error(None, &err, SyncErrorRow::ItemPatch { hunk }),
        }
    }

    /// Records the failure of a store step over `collection`, `None` for a
    /// store-wide one.
    pub fn note_step_failure(
        &mut self,
        step: FailedStep,
        collection: Option<String>,
        err: &anyhow::Error,
    ) {
        self.errors.push(SyncError::step(step, collection, err));
    }

    /// Records the endpoint `endpoint` unreached for `failure`.
    pub fn note_unreached(&mut self, endpoint: String, failure: Failure) {
        let row = SyncErrorRow::Unreached(UnreachedEndpoint { endpoint });
        self.errors.push(SyncError::new(None, failure, row));
    }

    /// Records a submit intent sent, or the failure that kept it from
    /// being sent among the errors, parked or left pending.
    pub fn note_submitted(&mut self, entry: SubmitEntry, outcome: Option<SubmitFailure>) {
        match outcome {
            None => self.submitted.push(entry),
            Some(SubmitFailure {
                failure,
                parked,
                unconfirmed,
            }) => {
                let row = SyncErrorRow::Submitted(SubmitError {
                    row: entry.id,
                    collection: entry.collection,
                    subject: entry.subject,
                    parked,
                    unconfirmed,
                });
                self.errors.push(SyncError::new(None, failure, row));
            }
        }
    }

    /// Records an intent performed, or the failure that kept it from being
    /// performed among the errors, parked or left pending.
    pub fn note_intent(&mut self, entry: IntentEntry, outcome: Option<(Failure, bool)>) {
        match outcome {
            None => self.intents.push(entry),
            Some((failure, parked)) => {
                let id = entry.seq.map(|seq| seq.to_string());
                let row = SyncErrorRow::Intents(IntentError {
                    row: entry.id,
                    collection: entry.collection,
                    intent: entry.kind,
                    source: entry.source,
                    parked,
                });
                self.errors
                    .push(SyncError::new(id.as_deref(), failure, row));
            }
        }
    }

    /// Records `err` as the error of the item `id` the row names.
    pub fn note_error(&mut self, id: Option<&str>, err: &anyhow::Error, row: SyncErrorRow) {
        self.errors.push(SyncError::new(id, Failure::of(err), row));
    }

    /// The writes a remote refused, each with its failure.
    pub fn rejected(&self) -> impl Iterator<Item = (&ItemError<ErrorCode>, &RejectedWrite)> {
        self.errors.iter().filter_map(|error| match &error.row {
            SyncErrorRow::Rejected(rejected) => Some((&error.failure, rejected)),
            _ => None,
        })
    }

    /// The changes a remote refused as they stand, each with its failure,
    /// the reason the store keeps as its message.
    pub fn refusals(&self) -> impl Iterator<Item = (&ItemError<ErrorCode>, &RefusedChange)> {
        self.errors.iter().filter_map(|error| match &error.row {
            SyncErrorRow::Refusals(refused) => Some((&error.failure, refused)),
            _ => None,
        })
    }

    /// Whether the run left work no rerun clears, which the exit code answers.
    ///
    /// A pending decision, a refused `UID`, a rejected or refused write, a
    /// parked send, intent or action, a parked absence and an unservable body
    /// wait for a person, none a run failure: a rerun alone meets each again.
    pub fn left_waiting(&self) -> bool {
        self.outstanding_conflicts > 0
            || !self.absences.is_empty()
            || !self.unreadable.is_empty()
            || !self.parked.is_empty()
            || self
                .errors
                .iter()
                .any(|error| error.row.waits_for_a_person())
    }

    /// Whether the run left work a rerun picks up, as an unreachable server
    /// reads: a run with nothing to do never carries an error.
    ///
    /// An error no person settles, a throttled source, an unsettled
    /// collection, an unanswered push.
    pub fn incomplete(&self) -> bool {
        self.interrupted
            || !self.throttled.is_empty()
            || !self.unanswered.is_empty()
            || !self.unsettled.is_empty()
            || self
                .coverage
                .iter()
                .any(|coverage| coverage.round.is_some() && !self.parks(coverage))
            || self
                .errors
                .iter()
                .any(|error| !error.row.waits_for_a_person())
    }
}

impl SyncOutput {
    /// Whether `coverage`'s round is open because its listing parked a mass
    /// absence or an emptied collection: it waits for a person, not a rerun.
    fn parks(&self, coverage: &CollectionCoverage) -> bool {
        self.absences.iter().any(|parked| {
            parked.side == coverage.source && parked.collection == coverage.collection
        })
    }
}

/// A listing that found most of a collection gone, or no member where it
/// held some, and dropped nothing (pimdir SYNC §5, §8).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParkedAbsence {
    /// The side whose listing it was.
    pub side: String,
    /// The collection, as the side names it.
    pub collection: String,
    /// The bound items the listing left out.
    pub absent: usize,
    /// The bound items counted: those in the round's scope, or the whole
    /// collection for a rebuild.
    pub bound: usize,
    /// Whether it was the rebuild a renumbered collection asks for, rather
    /// than a round of a sync.
    pub rebuild: bool,
}

impl fmt::Display for ParkedAbsence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            collection,
            absent,
            bound,
            rebuild,
        } = self;
        let listing = if *rebuild {
            "the rebuild of"
        } else {
            "the listing of"
        };
        write!(
            f,
            "{listing} {collection} on {side} left out {absent} of {bound} items, so nothing was dropped; once the server is known to hold no more, sync again with --accept-absences"
        )
    }
}

/// The changes one side withheld over bodies the store could not serve.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnreadableBodies {
    /// The side the changes were for.
    pub side: String,
    /// The collection, as the side names it.
    pub collection: String,
    /// How many changes were withheld.
    pub count: usize,
}

impl fmt::Display for UnreadableBodies {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            collection,
            count,
        } = self;
        write!(
            f,
            "{count} change(s) to {collection} on {side} withheld: their body is missing from the store or does not match its name"
        )
    }
}

/// A body a policy did not adopt, kept as a retained item.
///
/// Settled by a side's write, which `side` and `id` name, or by the drain
/// applying a queued action, which `action` names.
#[derive(Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OverruledBody {
    /// The side whose write settled it.
    pub side: Option<String>,
    /// The collection: as the side names it, else as the store does.
    pub collection: String,
    /// The item's handle on that side.
    pub id: Option<String>,
    /// The queue row whose apply settled it.
    #[serde(rename = "row")]
    pub action: Option<i64>,
    /// The key of the retained item keeping the losing body.
    pub kept: String,
}

impl fmt::Display for OverruledBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            collection,
            id,
            action,
            kept,
        } = self;
        match (side, id, action) {
            (Some(side), Some(id), _) => write!(f, "item {id} in {collection} on {side}")?,
            (_, _, Some(action)) => write!(f, "queued action #{action} in {collection}")?,
            _ => write!(f, "an item in {collection}")?,
        }
        write!(
            f,
            " settled by policy, the other body kept as retained item {kept}"
        )
    }
}

/// The pushes one side's server gave no outcome for.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnansweredPushes {
    /// The side the pushes were for.
    pub side: String,
    /// The collection, as the side names it.
    pub collection: String,
    /// How many pushes the last pass left unanswered.
    pub count: usize,
}

impl fmt::Display for UnansweredPushes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            collection,
            count,
        } = self;
        write!(
            f,
            "{count} change(s) to {collection} on {side} got no answer from the server, pushed again by the next run"
        )
    }
}

/// The members a capped source left unlisted in a round that closed, each
/// kept (pimdir SYNC §5).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnlistedMembers {
    /// The side configured as capped.
    pub side: String,
    /// The collection, as the side names it.
    pub collection: String,
    /// How many members the store binds there that the round did not list.
    pub count: usize,
}

impl fmt::Display for UnlistedMembers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            collection,
            count,
        } = self;
        write!(
            f,
            "{count} item(s) of {collection} not listed by {side}, which is capped: kept"
        )
    }
}

/// An open series a source read from a floor later than its start.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClippedSeries {
    /// The side that read the series.
    pub side: String,
    /// The collection, as the side names it.
    pub collection: String,
    /// The series, by its handle on the side.
    pub id: String,
    /// The floor its exceptions were read from, RFC 3339 UTC.
    pub since: String,
}

impl fmt::Display for ClippedSeries {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            collection,
            id,
            since,
        } = self;
        write!(
            f,
            "series {id} in {collection} on {side} starts before {since}, so the exceptions it holds before that were not read and its body was synced without them"
        )
    }
}

/// The writes of one action a source's configuration forbids in one
/// collection.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForbiddenWrites {
    /// The source the writes were for.
    pub side: String,
    /// The collection, as the side names it.
    pub collection: String,
    /// The action its configuration forbids: `create`, `update`, `delete`
    /// or `set-flags`.
    pub kind: String,
    /// How many writes it held back.
    pub count: usize,
}

impl fmt::Display for ForbiddenWrites {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            collection,
            kind,
            count,
        } = self;
        write!(
            f,
            "{count} {kind} write(s) to {collection} on {side} held back, its configuration forbids them"
        )
    }
}

/// A create a side's sync left waiting rather than pushing it (pimdir SYNC
/// §5).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WaitingCreate {
    /// The side the create is for.
    pub side: String,
    /// The collection, as the side names it.
    pub collection: String,
    /// The create's key, its link id: the side names no member for it yet.
    pub id: String,
    /// Why no add was pushed for it.
    pub reason: WaitReason,
}

impl fmt::Display for WaitingCreate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            collection,
            id,
            reason,
        } = self;
        write!(f, "{id} waits to reach {collection} on {side}: {reason}")
    }
}

/// Why a create waits (pimdir SYNC §2, §5).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum WaitReason {
    /// Its date lies outside the run's scope, and no page of that scope lists
    /// its arrival: it waits for a round over a scope holding it.
    OutOfScope,
    /// The server took it already, without a listing naming the member
    /// yet: it waits for the page that names it.
    Delivered,
    /// The store holds neither a body to upload nor a member to copy it
    /// from: it waits for its body.
    Undeliverable,
    /// A reason this neverest does not name.
    Other,
}

impl fmt::Display for WaitReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OutOfScope => "its date is outside the scope of this run",
            Self::Delivered => "the server took it, and no listing names it yet",
            Self::Undeliverable => "the store holds no body to upload for it",
            Self::Other => "it is not pushed yet",
        })
    }
}

/// A member a listing named apart from a create of its own side holding
/// its identity (pimdir SYNC §5, §6).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DisagreedMember {
    /// The side whose listing named the member.
    pub side: String,
    /// The collection, as the side names it.
    pub collection: String,
    /// The member, by the handle the side lists it under.
    pub id: String,
    /// The key of the create it was named apart from.
    pub create: String,
}

impl fmt::Display for DisagreedMember {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            side,
            collection,
            id,
            create,
        } = self;
        write!(
            f,
            "{id} in {collection} on {side} states the identity of the create {create} under another subject, date or sender, so it was kept apart from it and the item may now be held twice"
        )
    }
}

/// What a refresh did to the store's search index.
///
/// The index folds the store's change feed in batches until it is current
/// or a deadline passes; a refresh the deadline cut leaves it `behind`, and
/// the next refresh goes on from there.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IndexRefresh {
    /// Changes of the store folded in.
    pub folded: u64,
    /// Bodies extracted and indexed for the first time.
    pub indexed: u64,
    /// Changes of the store not folded in yet: 0 once the index is current.
    pub behind: u64,
    /// Whether another indexer held the index, so this one did nothing.
    pub busy: bool,
    /// The stable code of the failure, `null` when it refreshed or the
    /// failure carries none.
    pub code: Option<ErrorCode>,
    /// Why the refresh failed, the index left as it was, the outermost
    /// message.
    pub error: Option<String>,
    /// The messages of the failure's causes, outermost first.
    pub sources: Vec<String>,
}

impl fmt::Display for IndexRefresh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            folded,
            indexed,
            behind,
            busy,
            code: _,
            error,
            sources,
        } = self;
        if let Some(error) = error {
            let error = chain(error, sources);
            return write!(f, "search index not refreshed: {error}");
        }
        if *busy {
            return write!(f, "search index held by another indexer, not refreshed");
        }
        write!(f, "folded {folded} change(s), {indexed} new body(ies)")?;
        match behind {
            0 => write!(f, ", current"),
            n => write!(f, ", {n} change(s) behind, the next run goes on"),
        }
    }
}

/// One source's coverage of one collection.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CollectionCoverage {
    /// The source, as the account names it.
    pub source: String,
    /// The collection, as the source names it.
    pub collection: String,
    /// The floor of the last closed round's scope, RFC 3339 UTC; `null`
    /// when it had none, or no round ever closed.
    pub since: Option<String>,
    /// Its ceiling, exclusive; `null` when it had none.
    pub until: Option<String>,
    /// When that round closed; `null` while none ever did, the source
    /// holding no complete listing of the collection yet.
    pub closed_at: Option<String>,
    /// The round still open, which the next run resumes.
    pub round: Option<OpenRound>,
}

impl fmt::Display for CollectionCoverage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            source,
            collection,
            since,
            until,
            closed_at,
            round,
        } = self;
        let span = |since: &Option<String>, until: &Option<String>| match (since, until) {
            (None, None) => String::from("everything"),
            (Some(since), None) => format!("since {since}"),
            (None, Some(until)) => format!("before {until}"),
            (Some(since), Some(until)) => format!("from {since} to {until}"),
        };

        write!(f, "{source} {collection}: ")?;
        match closed_at {
            Some(at) => write!(f, "{} as of {at}", span(since, until))?,
            None => write!(f, "never listed whole")?,
        }
        if let Some(round) = round {
            write!(
                f,
                ", a round over {} open since {} to resume",
                span(&round.since, &round.until),
                round.started_at,
            )?;
        }
        Ok(())
    }
}

/// A round left open: its scope and when it opened.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenRound {
    /// The floor of the scope it lists.
    pub since: Option<String>,
    /// The ceiling of the scope it lists, exclusive.
    pub until: Option<String>,
    /// When it opened, RFC 3339 UTC.
    pub started_at: String,
}

/// The octets one source received this run.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceDownload {
    /// The source or target, as the account names it.
    pub source: String,
    /// The octets of bodies and listed meta.
    pub bytes: u64,
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

/// One thing a run could not do: pimalaya-cli's item error, then what
/// failed, tagged `kind`, with the keys that kind needs.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SyncError {
    /// The item's id when the failure is one item's, `null` for a hunk, an
    /// endpoint, a step or a send, then the code, message and causes.
    #[serde(flatten)]
    pub failure: ItemError<ErrorCode>,
    /// What failed.
    #[serde(flatten)]
    pub row: SyncErrorRow,
}

impl SyncError {
    /// The error of the item `id`, `failure` its code, message and causes.
    pub fn new(id: Option<&str>, failure: Failure, row: SyncErrorRow) -> Self {
        let Failure {
            code,
            error,
            sources,
        } = failure;

        Self {
            failure: ItemError {
                id: id.map(String::from),
                code,
                error,
                sources,
            },
            row,
        }
    }

    /// The failure `err` of a store step over `collection`, `None` for a
    /// store-wide one.
    pub fn step(step: FailedStep, collection: Option<String>, err: &anyhow::Error) -> Self {
        let row = SyncErrorRow::Failures(StepFailure { step, collection });
        Self::new(None, Failure::of(err), row)
    }

    /// The message and its causes, as the text output prints them.
    pub fn text(&self) -> String {
        chain(&self.failure.error, &self.failure.sources)
    }
}

/// The line the text output prints, as the run's warnings and errors list
/// each kind.
impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let error = self.text();
        let id = self.failure.id.as_deref().unwrap_or_default();

        match &self.row {
            SyncErrorRow::CollectionPatch { hunk } => write!(f, "{hunk}: {error}"),
            SyncErrorRow::ItemPatch { hunk } => write!(f, "{hunk}: {error}"),
            SyncErrorRow::Failures(StepFailure { step, collection }) => match collection {
                Some(collection) => write!(f, "{step} of {collection}: {error}"),
                None => write!(f, "{step}: {error}"),
            },
            SyncErrorRow::Rejected(RejectedWrite {
                side,
                collection,
                action,
            }) => write!(
                f,
                "{side} refused the {action} of {id} in {collection}, so it stays in the store: {error}"
            ),
            SyncErrorRow::Duplicates(RefusedDuplicate {
                side,
                collection,
                uid,
            }) => write!(
                f,
                "{side} refused a copy in {collection}: it already holds UID {uid}, so the second copy stays unwritten until one of the two carries a UID of its own"
            ),
            SyncErrorRow::Refusals(RefusedChange { side, collection }) => write!(
                f,
                "{side} refused the change of {id} in {collection} as it stands, held until the item changes: {error}"
            ),
            SyncErrorRow::Submitted(SubmitError {
                subject,
                parked,
                unconfirmed,
                ..
            }) => {
                let subject = subject.as_deref().unwrap_or("<no subject>");
                match (parked, unconfirmed) {
                    (true, true) => write!(
                        f,
                        "{subject} may have been sent, parked and never sent again: {error}"
                    ),
                    (true, false) => write!(f, "{subject} parked, never retried: {error}"),
                    (false, _) => {
                        write!(f, "{subject} not submitted, retried next run: {error}")
                    }
                }
            }
            SyncErrorRow::Intents(IntentError {
                row,
                intent,
                parked,
                ..
            }) => match parked {
                true => write!(f, "{intent} #{row} parked, never retried: {error}"),
                false => write!(
                    f,
                    "{intent} #{row} not performed, retried next run: {error}"
                ),
            },
            SyncErrorRow::Unreached(UnreachedEndpoint { endpoint }) => {
                write!(f, "{endpoint}: {error}")
            }
        }
    }
}

/// What a run could not do, `kind` the list the report held it in before
/// its failures were one list.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SyncErrorRow {
    /// A collection hunk that did not apply: a rerun retries it.
    CollectionPatch {
        /// The change the run tried to make.
        hunk: CollectionHunk,
    },
    /// An item hunk that did not apply: a rerun retries it.
    ItemPatch {
        /// The change the run tried to make.
        hunk: ItemHunk,
    },
    /// A store step that failed outside any hunk: a rerun retries it.
    Failures(StepFailure),
    /// A write a remote would not take, the change staying in the store:
    /// retried by every run, it waits for a person.
    Rejected(RejectedWrite),
    /// A create a side refused because it already holds the item's `UID`
    /// (RFC 4791 §5.3.2, RFC 6352 §6.3.2): it waits for a person.
    Duplicates(RefusedDuplicate),
    /// A change a remote refused as it stands, held while the refusal
    /// stands (pimdir SYNC §5): it waits for a person.
    Refusals(RefusedChange),
    /// A queued send that failed: parked, it waits for a person, else a
    /// rerun retries it.
    Submitted(SubmitError),
    /// A queued intent that failed: parked, it waits for a person, else a
    /// rerun retries it.
    Intents(IntentError),
    /// An endpoint whose connections the run could not open: a rerun
    /// retries it.
    Unreached(UnreachedEndpoint),
}

impl SyncErrorRow {
    /// Whether the row waits for a person (exit 3) rather than a rerun
    /// (exit 4).
    pub fn waits_for_a_person(&self) -> bool {
        match self {
            Self::Rejected(_) | Self::Duplicates(_) | Self::Refusals(_) => true,
            Self::Submitted(SubmitError { parked, .. }) => *parked,
            Self::Intents(IntentError { parked, .. }) => *parked,
            Self::CollectionPatch { .. }
            | Self::ItemPatch { .. }
            | Self::Failures(_)
            | Self::Unreached(_) => false,
        }
    }

    /// The endpoints the row names, to name them otherwise.
    pub fn endpoints_mut(&mut self) -> Vec<&mut String> {
        match self {
            Self::CollectionPatch { hunk } => hunk.endpoints_mut(),
            Self::ItemPatch { hunk } => hunk.endpoints_mut(),
            Self::Rejected(RejectedWrite { side, .. })
            | Self::Duplicates(RefusedDuplicate { side, .. })
            | Self::Refusals(RefusedChange { side, .. }) => vec![side],
            Self::Intents(IntentError { source, .. }) => vec![source],
            Self::Unreached(UnreachedEndpoint { endpoint }) => vec![endpoint],
            Self::Failures(_) | Self::Submitted(_) => Vec::new(),
        }
    }

    /// Whether the row is a failed hunk or step, which the text output
    /// counts among the errors rather than the warnings.
    fn is_error(&self) -> bool {
        match self {
            Self::CollectionPatch { .. }
            | Self::ItemPatch { .. }
            | Self::Failures(_)
            | Self::Submitted(_)
            | Self::Intents(_) => true,
            Self::Rejected(_) | Self::Duplicates(_) | Self::Refusals(_) | Self::Unreached(_) => {
                false
            }
        }
    }
}

/// An endpoint whose connections the run could not open.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnreachedEndpoint {
    /// The source or target, as the account names it, or `<source>.smtp`
    /// for a source's SMTP channel.
    pub endpoint: String,
}

/// One create refused for `no-uid-conflict` (RFC 4791 §5.3.2, RFC 6352
/// §6.3.2).
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

/// One change a remote refused as it stands (pimdir SYNC §4, §5), the
/// item's handle as the error's `id`, what the remote said its message.
///
/// Held on the item's binding while it stands, pushing nothing; changing the
/// item pushes it anew, removing it withdraws it. Reported by every run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefusedChange {
    /// The side that refused the change.
    pub side: String,
    /// The collection the item sits in, as the side names it.
    pub collection: String,
}

/// One write a remote would not take, or that never reached the wire, the
/// item's handle on that side as the error's `id`.
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
    /// What the run tried: `update`, `append`, `copy`, `delete`, `move`,
    /// `set-flags`.
    pub action: String,
}

/// One item both sides changed against a shared base, left conflicted.
///
/// The residue of the three-way merge, which already took both sides where
/// they touched different fields; whose edit wins is itself an edit, staged
/// through the pimdir queue. Only mutable-content kinds reach this state.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemConflict {
    /// The item's public id, the one `conflict show` and `conflict resolve`
    /// take, `null` when the store names the item by no id yet.
    pub id: Option<String>,
    /// The store collection the item sits in, as `conflict list` spells it.
    pub collection: String,
    /// The side the divergence is against, as the account names it.
    pub side: String,
    /// The item's handle on that side, its id on the remote.
    pub remote_id: String,
}

impl fmt::Display for ItemConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            id,
            collection,
            side,
            remote_id,
        } = self;
        match id {
            Some(id) => write!(f, "item {id} ({remote_id})")?,
            None => write!(f, "item {remote_id}")?,
        }
        write!(
            f,
            " in {collection} on {side} changed on both sides and is left conflicted"
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

/// One queue row the store holds parked, as `pimdir queue list` prints a
/// row in every binary reading a pimdir store.
///
/// It stays in the store until a person retries or cancels it, and every
/// run reports it again.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParkedQueueAction {
    /// The queue row's id.
    pub row: i64,
    /// The row under the store's identity, `<store-id>.<incarnation>.<row>`
    /// (pimdir STORAGE §4.5), as a frontend's `pimdir queue` takes it.
    pub handle: String,
    /// The action as the store spells it, kebab-case: `add`, `update`,
    /// `set-flags`, `remove`, `move`, `copy`, `submit`, `calendar-reply`…
    pub kind: String,
    /// The store collection the row is queued on.
    pub collection: String,
    /// The link id of the item the row creates, `null` when it creates none.
    pub link_id: Option<String>,
    /// The id of the stored item the row addresses, `null` when it
    /// addresses none.
    pub item_id: Option<String>,
    /// The enqueuing process, diagnostic only.
    pub producer: String,
    /// When the store stamped the row, RFC 3339.
    pub queued_at: String,
    /// How many times the row was tried before it was parked.
    pub attempts: i64,
    /// Whether the row's outcome is unknown (pimdir STORAGE §15.2), its
    /// `error` then beginning `unconfirmed: `.
    pub unconfirmed: bool,
    /// The failure that parked the row, as the store recorded it.
    pub error: String,
}

impl ParkedQueueAction {
    /// The row `parked` of the store `identity`, what it creates and
    /// addresses read from its payload, `null` where it does not decode.
    pub fn new(identity: &PimdirStoreIdentity, parked: PimdirParkedAction) -> Self {
        let handle = PimdirActionHandle {
            id: parked.id,
            store: Some(identity.clone()),
        };
        let (link_id, item_id) = match action_from_payload(&parked.action, &parked.payload) {
            Ok(PimdirAction::Add { link_id, .. }) => (link_id.map(|id| id.0), None),
            Ok(
                PimdirAction::Update { seq, .. }
                | PimdirAction::SetFlags { seq, .. }
                | PimdirAction::Remove { seq, .. }
                | PimdirAction::Move { seq, .. }
                | PimdirAction::Copy { seq, .. },
            ) => (None, Some(seq.to_string())),
            Ok(PimdirAction::Unknown { kind, payload, .. }) => {
                match PimdirInvitation::from_payload(&kind, &payload).map(|it| it.item) {
                    Ok(PimdirIntentItem::Seq(seq)) => (None, Some(seq.to_string())),
                    Ok(PimdirIntentItem::Link(link_id)) => (Some(link_id.0), None),
                    Err(_) => (None, None),
                }
            }
            Ok(PimdirAction::SetPerformer { .. }) | Err(_) => (None, None),
        };

        Self {
            row: parked.id,
            handle: handle.to_string(),
            unconfirmed: parked.unconfirmed().is_some(),
            kind: parked.action,
            collection: parked.collection,
            link_id,
            item_id,
            producer: parked.producer,
            queued_at: parked.created_at,
            attempts: parked.attempts,
            error: parked.error,
        }
    }
}

impl fmt::Display for ParkedQueueAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            row,
            kind,
            collection,
            producer,
            error,
            ..
        } = self;
        write!(
            f,
            "parked queue action #{row} ({kind} in {collection} from {producer}): {error}"
        )
    }
}

/// One submit intent this run sent, its row dropped and its pin released.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmitEntry {
    /// The queue row's global append id.
    #[serde(rename = "row")]
    pub id: i64,
    /// The collection the intent was anchored on.
    pub collection: String,
    /// The submitted item's subject, when the intent payload carried one.
    pub subject: Option<String>,
}

impl fmt::Display for SubmitEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let subject = self.subject.as_deref().unwrap_or("<no subject>");
        write!(f, "submitted {subject}")
    }
}

/// How a submit intent failed: parked when permanent or of unknown
/// outcome (pimdir STORAGE §13), pending otherwise.
#[derive(Clone, Debug)]
pub struct SubmitFailure {
    /// The send error.
    pub failure: Failure,
    /// Whether the failure parked the row rather than leaving it pending.
    pub parked: bool,
    /// Whether the message may have been delivered despite the failure.
    pub unconfirmed: bool,
}

/// A submit intent that failed, as the errors list holds it.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmitError {
    /// The queue row's global append id.
    pub row: i64,
    /// The collection the intent was anchored on.
    pub collection: String,
    /// The item's subject, when the intent payload carried one.
    pub subject: Option<String>,
    /// Whether the failure parked the row rather than leaving it pending.
    pub parked: bool,
    /// Whether the message may have been delivered despite the failure (no
    /// verdict, a gateway error, a stopped run), false otherwise.
    ///
    /// Such a row is parked unconfirmed (pimdir STORAGE §15.2), never sent
    /// again on its own: a person retries or withdraws it.
    pub unconfirmed: bool,
}

/// One calendar or collection intent this run performed (pimdir STORAGE
/// Annex B.2), its queue row dropped.
///
/// The effect itself, the new `PARTSTAT` or the removal, arrives with the
/// next sync.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntentEntry {
    /// The queue row's global append id.
    #[serde(rename = "row")]
    pub id: i64,
    /// The collection of the item it addresses.
    pub collection: String,
    /// The intent kind: `calendar-reply`, `calendar-cancel`,
    /// `collection-create`…
    pub kind: String,
    /// The source that performed it.
    pub source: String,
    /// The public id of the item, when the payload named one.
    pub seq: Option<i64>,
}

impl fmt::Display for IntentEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            id, kind, source, ..
        } = self;
        write!(f, "{kind} #{id} performed by {source}")
    }
}

/// An intent that failed, as the errors list holds it, the public id of
/// the item it addresses as the error's `id`.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntentError {
    /// The queue row's global append id.
    pub row: i64,
    /// The collection of the item it addresses.
    pub collection: String,
    /// The intent kind: `calendar-reply`, `calendar-cancel`,
    /// `collection-create`…
    pub intent: String,
    /// The source that tried to perform it.
    pub source: String,
    /// Whether the failure parked the row rather than leaving it pending.
    pub parked: bool,
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

/// A step of the run that failed outside any hunk.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StepFailure {
    /// The step that failed.
    pub step: FailedStep,
    /// The collection the step worked on, `null` for a store-wide one.
    pub collection: Option<String>,
}

/// A store step a run takes outside any hunk.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FailedStep {
    /// Reading or draining the queue frontends filled.
    Queue,
    /// Landing downloaded bodies in the store.
    Bodies,
    /// Counting the conflicts waiting for a decision.
    Conflicts,
    /// Listing a collection, which the server cut short.
    Listing,
}

impl fmt::Display for FailedStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Queue => write!(f, "queue"),
            Self::Bodies => write!(f, "bodies"),
            Self::Conflicts => write!(f, "conflict count"),
            Self::Listing => write!(f, "listing"),
        }
    }
}

impl fmt::Display for SyncOutput {
    /// Prints what is worth a line: a collection covered whole prints none,
    /// nor an index refresh with nothing to fold (a failed one is a warning).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f)?;

        let failed_collections: Vec<&CollectionHunk> = self
            .errors
            .iter()
            .filter_map(|error| match &error.row {
                SyncErrorRow::CollectionPatch { hunk } => Some(hunk),
                _ => None,
            })
            .collect();
        let failed_items: Vec<&ItemHunk> = self
            .errors
            .iter()
            .filter_map(|error| match &error.row {
                SyncErrorRow::ItemPatch { hunk } => Some(hunk),
                _ => None,
            })
            .collect();
        let of = |kind: fn(&SyncErrorRow) -> bool| -> Vec<&SyncError> {
            self.errors
                .iter()
                .filter(|error| kind(&error.row))
                .collect()
        };
        let failed_sends = of(|row| matches!(row, SyncErrorRow::Submitted(_)));
        let failed_intents = of(|row| matches!(row, SyncErrorRow::Intents(_)));
        let warned = of(|row| {
            matches!(
                row,
                SyncErrorRow::Rejected(_) | SyncErrorRow::Duplicates(_) | SyncErrorRow::Refusals(_)
            )
        });
        let unreached = of(|row| matches!(row, SyncErrorRow::Unreached(_)));
        let failed_steps = of(|row| {
            matches!(
                row,
                SyncErrorRow::CollectionPatch { .. }
                    | SyncErrorRow::ItemPatch { .. }
                    | SyncErrorRow::Failures(_)
            )
        });

        let hunk_errors = failed_collections.len() + failed_items.len();
        let total = self.collection_patch.len() + self.item_patch.len() + hunk_errors;
        let errors = of(SyncErrorRow::is_error).len();
        let warnings = self.collisions.len()
            + self.parked.len()
            + self.conflicts.len()
            + warned.len()
            + self.absences.len()
            + self.unreadable.len()
            + self.overruled.len()
            + self.unanswered.len()
            + self.disagreed.len()
            + self.clipped.len()
            + self
                .index
                .as_ref()
                .map_or(0, |index| usize::from(index.error.is_some()));

        if !self.drained.is_empty() {
            writeln!(f, "Queue ({n}):", n = self.drained.len())?;
            for entry in &self.drained {
                writeln!(f, " - {entry}")?;
            }
            writeln!(f)?;
        }

        if !self.submitted.is_empty() || !failed_sends.is_empty() {
            let n = self.submitted.len() + failed_sends.len();
            writeln!(f, "Submissions ({n}):")?;
            for entry in &self.submitted {
                writeln!(f, " - {entry}")?;
            }
            for error in failed_sends {
                writeln!(f, " - {error}")?;
            }
            writeln!(f)?;
        }

        if !self.intents.is_empty() || !failed_intents.is_empty() {
            let n = self.intents.len() + failed_intents.len();
            writeln!(f, "Intents ({n}):")?;
            for entry in &self.intents {
                writeln!(f, " - {entry}")?;
            }
            for error in failed_intents {
                writeln!(f, " - {error}")?;
            }
            writeln!(f)?;
        }

        if !self.collection_patch.is_empty() || !failed_collections.is_empty() {
            let n = self.collection_patch.len() + failed_collections.len();
            writeln!(f, "Collection patches ({n}):")?;
            for hunk in self.collection_patch.iter().chain(failed_collections) {
                writeln!(f, " - {hunk}")?;
            }
            writeln!(f)?;
        }

        if !self.item_patch.is_empty() || !failed_items.is_empty() {
            let n = self.item_patch.len() + failed_items.len();
            writeln!(f, "Item patches ({n}):")?;
            for hunk in self.item_patch.iter().chain(failed_items) {
                writeln!(f, " - {hunk}")?;
            }
            writeln!(f)?;
        }

        let bounded: Vec<&CollectionCoverage> = self
            .coverage
            .iter()
            .filter(|c| c.since.is_some() || c.until.is_some() || c.round.is_some())
            .collect();
        if !bounded.is_empty() {
            writeln!(f, "Coverage ({n}):", n = bounded.len())?;
            for coverage in bounded {
                writeln!(f, " - {coverage}")?;
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

        if let Some(index) = &self.index
            && index.error.is_none()
            && (index.folded > 0 || index.behind > 0 || index.busy)
        {
            writeln!(f, "Search index:")?;
            writeln!(f, " - {index}")?;
            writeln!(f)?;
        }

        if !self.forbidden.is_empty() {
            writeln!(f, "Forbidden ({n}):", n = self.forbidden.len())?;
            for forbidden in &self.forbidden {
                writeln!(f, " - {forbidden}")?;
            }
            writeln!(f)?;
        }

        if !self.waiting.is_empty() {
            writeln!(f, "Waiting ({n}):", n = self.waiting.len())?;
            for waiting in &self.waiting {
                writeln!(f, " - {waiting}")?;
            }
            writeln!(f)?;
        }

        if !self.unlisted.is_empty() {
            writeln!(f, "Unlisted ({n}):", n = self.unlisted.len())?;
            for unlisted in &self.unlisted {
                writeln!(f, " - {unlisted}")?;
            }
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
            for error in warned {
                writeln!(f, " - {error}")?;
            }
            for p in &self.parked {
                writeln!(f, " - {p}")?;
            }
            for a in &self.absences {
                writeln!(f, " - {a}")?;
            }
            for u in &self.unreadable {
                writeln!(f, " - {u}")?;
            }
            for o in &self.overruled {
                writeln!(f, " - {o}")?;
            }
            for u in &self.unanswered {
                writeln!(f, " - {u}")?;
            }
            for d in &self.disagreed {
                writeln!(f, " - {d}")?;
            }
            for c in &self.clipped {
                writeln!(f, " - {c}")?;
            }
            if let Some(index) = &self.index
                && index.error.is_some()
            {
                writeln!(f, " - {index}")?;
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

        if self.interrupted {
            writeln!(
                f,
                "Interrupted: a signal stopped the run, rerun it to go on"
            )?;
            writeln!(f)?;
        }

        if !self.unsettled.is_empty() {
            writeln!(f, "Unsettled ({n}):", n = self.unsettled.len())?;
            for collection in &self.unsettled {
                writeln!(f, " - {collection} was still moving when the run stopped")?;
            }
            writeln!(f)?;
        }

        if !unreached.is_empty() {
            writeln!(f, "Unreached ({n}):", n = unreached.len())?;
            for error in unreached {
                writeln!(f, " - {error}")?;
            }
            writeln!(f)?;
        }

        if !self.throttled.is_empty() {
            writeln!(f, "Throttled ({n}):", n = self.throttled.len())?;
            for throttled in &self.throttled {
                writeln!(
                    f,
                    " - {} gave up throttled, rerun it after {}",
                    throttled.source, throttled.until
                )?;
            }
            writeln!(f)?;
        }

        if errors > 0 {
            writeln!(f, "Errors ({errors}):")?;
            for error in failed_steps {
                writeln!(f, " - {error}")?;
            }
            writeln!(f)?;
        }

        let account = &self.account;
        let stopped = errors == 0 && self.incomplete();
        match (total, errors, warnings, self.dry_run) {
            (0, 0, 0, _) if stopped => {
                writeln!(f, "Account {account} is incomplete, rerun it to go on")
            }
            (0, 0, w, _) if stopped => writeln!(
                f,
                "Account {account} is incomplete ({w} warnings), rerun it to go on"
            ),
            (n, 0, 0, false) if stopped => writeln!(
                f,
                "Account {account} partially synchronized: {n} hunks, rerun it to go on"
            ),
            (n, 0, w, false) if stopped => writeln!(
                f,
                "Account {account} partially synchronized: {n} hunks, {w} warnings, rerun it to go on"
            ),
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
            (n, e, w, false) => {
                let applied = n - hunk_errors;
                let warned = match w {
                    0 => String::new(),
                    w => format!(", {w} warnings"),
                };
                match applied {
                    0 => writeln!(f, "Account {account} not synchronized: {e} errors{warned}"),
                    n => writeln!(
                        f,
                        "Account {account} partially synchronized: {n} hunks, {e} errors{warned}"
                    ),
                }
            }
        }
    }
}

/// What `sync --index-only` did to one store's search index.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IndexOutput {
    /// The store directory whose index was built.
    pub store: PathBuf,
    /// The accounts of the run using the store, in name order.
    pub accounts: Vec<String>,
    /// What the build folded, and whether another indexer held it.
    pub index: IndexRefresh,
}

impl fmt::Display for IndexOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            store,
            accounts,
            index,
        } = self;
        writeln!(
            f,
            "Search index of {} ({}): {index}",
            store.display(),
            accounts.join(", ")
        )
    }
}
