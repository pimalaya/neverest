//! # Pair run
//!
//! A source against each target of its account: the collection pair loop and
//! the reconcile rounds that converge one collection's two sides.

use crate::sync::driver::*;

/// The one kind a two-side account syncs, refusing a disagreeing pair.
///
/// The kind is never configured, falling out of each side's backend, so this is
/// the only enforcement point. It runs as soon as the media types are known and
/// before the first store write, so a mismatch fails with nothing half-written.
fn check_kinds(left: &mut SourceCtx, right: &mut SourceCtx) -> Result<Kind> {
    let left = (
        left.name.clone(),
        left.pool.primary().media_type().to_string(),
    );
    let right = (
        right.name.clone(),
        right.pool.primary().media_type().to_string(),
    );
    let left = (left.0.as_str(), left.1.as_str());
    let right = (right.0.as_str(), right.1.as_str());
    pair_kind(left, right)
}

/// The pure half of [`check_kinds`]: two media types in, the agreed kind out.
pub(super) fn pair_kind(left: (&str, &str), right: (&str, &str)) -> Result<Kind> {
    let resolve = |(source, raw): (&str, &str)| -> Result<Kind> {
        Kind::from_media_type(raw).with_context(|| {
            format!("This build cannot sync items of type {raw} (source {source})")
        })
    };

    let left_kind = resolve(left)?;
    let right_kind = resolve(right)?;

    if left_kind != right_kind {
        bail!(
            "The two sides sync different kinds and cannot be reconciled: \
             {} is {} and {} is {}. \
             Pair each kind with its own account (they may share a `store.root`).",
            left.0,
            left_kind.media_type(),
            right.0,
            right_kind.media_type(),
        );
    }

    Ok(left_kind)
}

/// Live per-collection progress of a pair, as `collection-progressed` events.
pub(super) struct CollectionProgress<'a> {
    pub(super) progress: Progress<'a>,
    pub(super) gate: PercentGate,
    pub(super) source: &'a str,
    pub(super) target: &'a str,
    pub(super) collection: &'a str,
}

impl CollectionProgress<'_> {
    /// Reports `done` of `total`, an event printed once per percent.
    pub(super) fn tick(&self, done: usize, total: usize) {
        if self.gate.passes(done, total) {
            self.progress.emit(|| SyncEvent::CollectionProgressed {
                source: self.source.to_owned(),
                target: self.target.to_owned(),
                collection: self.collection.to_owned(),
                done,
                total,
            });
        }
    }
}

/// What a pair run reads of its account: the account, its mode and the
/// run's options, the same for every pair of the run.
#[derive(Clone, Copy)]
pub(super) struct AccountRun<'a> {
    pub(super) name: &'a str,
    pub(super) config: &'a AccountConfig,
    pub(super) account: &'a Account,
    pub(super) mode: &'a AccountMode,
    pub(super) scope: &'a PimdirScope,
    pub(super) work_dir: &'a Path,
    pub(super) dry_run: bool,
    pub(super) accept_absences: bool,
    pub(super) connections: usize,
    pub(super) progress: Progress<'a>,
}

/// One source against every target the account names, each pairing on its own.
///
/// Both endpoints bind the source's namespace, which is what makes them meet.
/// Targets run in turn rather than together, no target influencing another
/// under `one-way`, so the source is enumerated once per target.
pub(super) fn run_targets(
    run: AccountRun<'_>,
    endpoints: &HashMap<String, SourceConfig>,
    source_name: &str,
    source_config: SourceConfig,
    collection_filter: Option<CollectionFilter>,
    state: &mut AccountState,
    report: &mut SyncOutput,
) -> Result<()> {
    let relay = run.mode.streams(endpoints);
    let mut taskless = PairTaskless::new(state, source_name);

    for target_name in &run.mode.targets {
        let pairing = Pairing {
            left: (source_name.to_string(), source_config.clone()),
            right: (target_name.clone(), endpoints[target_name].clone()),
            filter: collection_filter.clone(),
            relay,
        };
        run_pair(run, pairing, state, &mut taskless, report)?;
    }

    record_taskless(state, source_name, &taskless.listed);
    Ok(())
}

/// One pairing of a run: its two endpoints, the source first, the filter
/// the run names, and whether bodies relay.
struct Pairing {
    left: (String, SourceConfig),
    right: (String, SourceConfig),
    filter: Option<CollectionFilter>,
    relay: bool,
}

/// What the pairs of one source stated of tasks this run, as hub ids.
///
/// The record before the run is kept while a target is still to list.
pub(super) struct PairTaskless {
    previous: BTreeSet<String>,
    listed: BTreeSet<String>,
}

impl PairTaskless {
    pub(super) fn new(state: &AccountState, source: &str) -> Self {
        Self {
            previous: state.taskless.get(source).cloned().unwrap_or_default(),
            listed: BTreeSet::new(),
        }
    }
}

/// Records what one pair's listings state of tasks, registers `common` and
/// declares the account again, before any item of the pair moves.
///
/// A task in a hub calendar crosses, so a calendar either endpoint keeps from
/// tasks takes none (pimdir STORAGE §15.6). Until every target has listed the
/// record only grows, so a run stopping early drops nothing.
pub(super) fn record_pair_listing(
    declaring: &Declaring<'_>,
    state: &mut AccountState,
    namespace: &str,
    taskless: &mut PairTaskless,
    listings: [&[Collection]; 2],
    common: &[String],
    dry_run: bool,
) {
    for listed in listings {
        taskless.listed.extend(taskless_ids(namespace, listed));
    }
    let known = taskless.previous.union(&taskless.listed).cloned().collect();
    record_taskless(state, namespace, &known);

    if !dry_run {
        declaring.register(namespace, common);
        declaring.again(state);
    }
}

/// One source against one target, propagation falling out of the shared hub.
///
/// Full hydration follows `retain`, never `relay`. Under `one-way` the target
/// follows the source, so no side records a conflict. A collection deleted on
/// both leaves the store, so one created again reads as new, not as deleted.
fn run_pair(
    run: AccountRun<'_>,
    pairing: Pairing,
    state: &mut AccountState,
    taskless: &mut PairTaskless,
    report: &mut SyncOutput,
) -> Result<()> {
    let filter = match pairing.filter.clone() {
        Some(filter) => filter,
        None => pairing.left.1.collection().filter.clone(),
    };
    let narrowing = pairing.right.1.collection().filter.clone();
    let mut pair = OpenPair::open(run, pairing)?;

    if !run.dry_run {
        pair.drain_queues(run, report);
    }
    let filters = [&filter, &narrowing];
    let common = pair.plan_collections(run, filters, state, taskless, report)?;
    pair.sync_collections(run, &common, report);

    Ok(())
}

/// The two sides of a pair, opened, with their stores.
struct OpenPair {
    /// The namespace both sides bind, the source's name.
    namespace: String,
    left: SourceCtx,
    right: SourceCtx,
    left_store: PimdirSourceStore,
    right_store: PimdirSourceStore,
    blobs: PimdirBlobStage,
    /// The media type both sides hold.
    media_type: &'static str,
    relay: bool,
    /// The collections each side's store calls settled, read when the
    /// pair's collections begin and again after a change.
    settled: [Settled; 2],
}

impl OpenPair {
    /// Opens both stores and both sides, checks they hold the same kind,
    /// and tells the progress each endpoint is open.
    fn open(run: AccountRun<'_>, pairing: Pairing) -> Result<Self> {
        let Pairing {
            left: (left_name, left_config),
            right: (right_name, right_config),
            relay,
            ..
        } = pairing;
        let namespace = left_name.clone();

        let left_store = open_store(run.work_dir, &left_name, run.name)?;
        let right_store = open_store(run.work_dir, &right_name, run.name)?;
        let blobs = left_store.blob_stage();

        let (left_authority, right_authority) = if run.mode.one_way {
            (Authority::Endpoint, Authority::Store)
        } else {
            (Authority::Shared, Authority::Shared)
        };
        let mut left = side(run, &namespace, &left_name, &left_config, left_authority)
            .with_context(|| Unreached::new(&left_name, format!("Open source {left_name}")))?;
        let mut right = side(run, &namespace, &right_name, &right_config, right_authority)
            .with_context(|| Unreached::new(&right_name, format!("Open target {right_name}")))?;
        for endpoint in [&left_name, &right_name] {
            run.progress.emit(|| SyncEvent::EndpointOpened {
                endpoint: endpoint.clone(),
            });
        }

        let media_type = check_kinds(&mut left, &mut right)?.media_type();

        Ok(Self {
            namespace,
            left,
            right,
            left_store,
            right_store,
            blobs,
            media_type,
            relay,
            settled: Default::default(),
        })
    }

    /// Performs what frontends queued: collection intents, sends and
    /// calendar intents.
    fn drain_queues(&mut self, run: AccountRun<'_>, report: &mut SyncOutput) {
        let Self {
            left,
            right,
            left_store,
            blobs,
            ..
        } = self;
        let renames =
            drain_collection_intents(&mut [&mut *left, &mut *right], left_store, true, report);
        drain_submits(
            run.config,
            run.account,
            &mut [SendSide::open(left), SendSide::open(right)],
            left_store,
            blobs,
            run.work_dir,
            &renames,
            report,
        );
        drain_invitations(&mut [left, right], left_store, run.work_dir, report);
    }

    /// Lists both sides, creates and deletes the collections one side holds
    /// alone as the rights allow, and answers the collections both hold.
    ///
    /// Only what both `filters` keep counts: the run's or the source's, then
    /// the target's, which narrows what the target takes. The collections
    /// both hold are declared, and the listings recorded, before any item moves.
    fn plan_collections(
        &mut self,
        run: AccountRun<'_>,
        filters: [&CollectionFilter; 2],
        state: &mut AccountState,
        taskless: &mut PairTaskless,
        report: &mut SyncOutput,
    ) -> Result<BTreeSet<String>> {
        let namespace = self.namespace.as_str();
        let (left_name, right_name) = (self.left.name.clone(), self.right.name.clone());

        let left_listed = self
            .left
            .pool
            .primary()
            .list_collections(false)
            .context("List collections error")?;
        let right_listed = self
            .right
            .pool
            .primary()
            .list_collections(false)
            .context("List collections error")?;
        let left_collections = names_by_id(&left_listed);
        let right_collections = names_by_id(&right_listed);
        for (endpoint, listed) in [
            (&left_name, &left_collections),
            (&right_name, &right_collections),
        ] {
            run.progress.emit(|| SyncEvent::EndpointListed {
                endpoint: endpoint.clone(),
                collections: listed.len(),
            });
        }

        let filtered = |collections: &BTreeMap<String, String>| {
            let [run_or_source, target] =
                filters.map(|filter| filter_collections(collections, filter));
            run_or_source
                .intersection(&target)
                .cloned()
                .collect::<BTreeSet<_>>()
        };
        let left_filtered = filtered(&left_collections);
        let right_filtered = filtered(&right_collections);

        let held = held_by_both(
            &self.left_store,
            namespace,
            left_filtered.symmetric_difference(&right_filtered),
            &left_name,
            &right_name,
        )?;
        let collection_hunks = diff_collections(
            &left_filtered,
            &right_filtered,
            &held,
            (&left_name, &self.left.perms.collection),
            (&right_name, &self.right.perms.collection),
        );
        for hunk in collection_hunks {
            let error = if run.dry_run {
                None
            } else {
                apply_collection_hunk(&hunk, &mut self.left, &mut self.right).err()
            };
            if !run.dry_run
                && error.is_none()
                && let CollectionHunk::Delete { collection, .. } = &hunk
            {
                let id = hub_id(namespace, collection);
                if let Err(err) = self.left_store.delete_collection(&id) {
                    warn!("cannot drop {id} from the store: {err}");
                }
            }
            report.note_collection_hunk(hunk, error);
        }

        let mut present_left = left_filtered.clone();
        let mut present_right = right_filtered.clone();
        for hunk in &report.collection_patch {
            if let CollectionHunk::Create { side, collection } = hunk {
                if *side == left_name {
                    present_left.insert(collection.clone());
                } else if *side == right_name {
                    present_right.insert(collection.clone());
                }
            }
        }
        let common: BTreeSet<String> = present_left.intersection(&present_right).cloned().collect();

        let mut syncing = Vec::with_capacity(common.len());
        for name in &common {
            let collection = hub_id(namespace, name);
            let known = self
                .left_store
                .ensure_collection(&collection, self.media_type);
            declare_name(
                &self.left_store,
                &collection,
                name,
                &[&left_collections, &right_collections],
            );
            match known {
                Ok(_) => syncing.push(collection),
                Err(err) => warn!("cannot declare the kind of {collection}: {err}"),
            }
        }
        let declaring = Declaring {
            dir: run.work_dir,
            account: run.name,
            config: run.config,
            mode: run.mode,
        };
        record_pair_listing(
            &declaring,
            state,
            namespace,
            taskless,
            [&left_listed, &right_listed],
            &syncing,
            run.dry_run,
        );

        Ok(common)
    }

    /// Syncs each collection both sides hold, in turn, until a stop; a
    /// collection that fails is reported and the next one runs.
    fn sync_collections(
        &mut self,
        run: AccountRun<'_>,
        common: &BTreeSet<String>,
        report: &mut SyncOutput,
    ) {
        let total = common.len();
        if !run.dry_run {
            let read = Settled::read(&self.left_store, run.work_dir)
                .and_then(|left| Ok([left, Settled::read(&self.right_store, run.work_dir)?]));
            match read {
                Ok(settled) => self.settled = settled,
                Err(err) => warn!("cannot read the settled collections, pushing each: {err:#}"),
            }
        }
        for (index, name) in common.iter().enumerate() {
            if crate::sync::stopping() {
                break;
            }
            self.sync_one(run, name, index + 1, total, report);
        }
    }

    /// Syncs one collection both sides hold, then hydrates it whole under
    /// `retain`, telling the progress where it starts and how it ends.
    fn sync_one(
        &mut self,
        run: AccountRun<'_>,
        name: &str,
        position: usize,
        total: usize,
        report: &mut SyncOutput,
    ) {
        let collection = &hub_id(&self.namespace, name);
        let (left_name, right_name) = (self.left.name.clone(), self.right.name.clone());
        let progress = run.progress;
        progress.emit(|| SyncEvent::CollectionStarted {
            source: left_name.clone(),
            target: right_name.clone(),
            collection: name.to_owned(),
            position,
            total,
        });
        let finished = |failure: Option<Failure>| {
            let (code, error, sources) = Failure::row(failure);
            SyncEvent::CollectionFinished {
                source: left_name.clone(),
                target: right_name.clone(),
                collection: name.to_owned(),
                code,
                error,
                sources,
            }
        };
        let ticks = CollectionProgress {
            progress,
            gate: PercentGate::default(),
            source: &left_name,
            target: &right_name,
            collection: name,
        };

        if let Err(err) = self.sync_collection(run, collection, &ticks, report) {
            warn!("{name} sync error: {err:#}");
            progress.emit(|| finished(Some(Failure::of(&err))));
            let hunk = CollectionHunk::Sync {
                source: left_name.clone(),
                target: right_name.clone(),
                collection: name.to_owned(),
            };
            report.note_collection_hunk(hunk, Some(err));
            return;
        }

        let mut error = None;
        if run.mode.retain
            && !run.dry_run
            && let Err(err) = hydrate_full_collection(
                collection,
                &mut self.left,
                &mut self.right,
                &mut self.left_store,
                &mut self.right_store,
                &self.blobs,
                &ticks,
            )
        {
            warn!("{name} full hydration error: {err:#}");
            error = Some(Failure::of(&err));
            report.note_step_failure(FailedStep::Bodies, Some(name.to_owned()), &err);
        }
        itemize_noted(&mut self.left, report);
        itemize_noted(&mut self.right, report);

        progress.emit(|| finished(error));
    }
}

/// One side of a pair, opened with the connections its backend may use.
fn side(
    run: AccountRun<'_>,
    namespace: &str,
    name: &str,
    config: &SourceConfig,
    authority: Authority,
) -> Result<SourceCtx> {
    let budget = connection_budget(config, run.connections);
    let account = run.account.get(name)?;
    Ok(SourceCtx {
        name: name.to_owned(),
        namespace: namespace.to_owned(),
        perms: config.permissions(),
        authority,
        pool: Pool::open(account, budget)?,
        refused: Vec::new(),
        rejected: Vec::new(),
        scope: run.scope.clone(),
        dry_run: run.dry_run,
        accept_absences: run.accept_absences,
        capped: config.collection().capped,
        noted: Noted::default(),
    })
}

impl OpenPair {
    /// Reconciles one collection until quiescent.
    ///
    /// Its own report is folded into the account's ([`SyncOutput::absorb`])
    /// as the one-source path folds it, so no arm is dropped; on failure too,
    /// what it reported before stopping having still happened.
    fn sync_collection(
        &mut self,
        run: AccountRun<'_>,
        collection: &str,
        progress: &CollectionProgress,
        account: &mut SyncOutput,
    ) -> Result<()> {
        self.left_store
            .ensure_collection(collection, self.media_type)
            .with_context(|| format!("Declare kind for {collection}"))?;

        let mut report = SyncOutput::default();
        let outcome = self.sync_collection_into(run, collection, progress, &mut report);

        report_coverage(&self.left_store, collection, &self.left, &mut report);
        report_coverage(&self.right_store, collection, &self.right, &mut report);
        itemize_noted(&mut self.left, &mut report);
        itemize_noted(&mut self.right, &mut report);

        account.absorb(report);

        outcome
    }

    /// The body of [`sync_collection`](Self::sync_collection), filling that
    /// collection's own report.
    ///
    /// The opening round only pulls, so its events are the remotes' own rather
    /// than the echo of this run's writes, and no push clears a projection
    /// before `itemize` reads it.
    fn sync_collection_into(
        &mut self,
        run: AccountRun<'_>,
        collection: &str,
        progress: &CollectionProgress,
        report: &mut SyncOutput,
    ) -> Result<()> {
        let (_, quiet) = self.reconcile_pass(run, collection, false, report)?;
        let quiet = quiet && self.propagate(collection, progress, report)? == 0;

        itemize(
            collection,
            [&self.left_store, &self.right_store],
            &self.left,
            &self.right,
            report,
        )?;
        if run.dry_run {
            return Ok(());
        }

        let [left, right] = &self.settled;
        if !quiet {
            left.changed();
            right.changed();
        }
        let dir = run.work_dir;
        let mut done = left.owes_no_push(collection, &self.left, &self.left_store, dir, quiet)?
            && right.owes_no_push(collection, &self.right, &self.right_store, dir, quiet)?;
        for _ in 0..=MAX_EXTRA_PASSES {
            if done {
                break;
            }
            let (progressed, quiet) = self.reconcile_pass(run, collection, true, report)?;
            let propagated = self.propagate(collection, progress, report)?;
            if !quiet || propagated > 0 {
                let [left, right] = &self.settled;
                left.changed();
                right.changed();
            }
            if !progressed && propagated == 0 {
                done = true;
                break;
            }
        }
        if !done {
            report.unsettled.push(collection.to_owned());
        }

        let (left, right) = (&mut self.left, &mut self.right);
        itemize_refused(&left.name, mem::take(&mut left.refused), report);
        itemize_refused(&right.name, mem::take(&mut right.refused), report);
        itemize_rejected(&left.name, mem::take(&mut left.rejected), report);
        itemize_rejected(&right.name, mem::take(&mut right.rejected), report);

        Ok(())
    }

    /// Propagates the collection's cross-side copies, returning how many
    /// moved.
    ///
    /// Retaining hydrates the body into the store for the projection to
    /// push; relaying streams it server-to-server and keeps only the spine.
    fn propagate(
        &mut self,
        collection: &str,
        progress: &CollectionProgress,
        report: &mut SyncOutput,
    ) -> Result<usize> {
        let Self {
            left,
            right,
            left_store,
            right_store,
            blobs,
            relay,
            ..
        } = self;
        if *relay {
            relay_copies(
                collection,
                left,
                right,
                left_store,
                right_store,
                progress,
                report,
            )
        } else {
            hydrate_copies(
                collection,
                left,
                right,
                left_store,
                right_store,
                blobs,
                progress,
            )
        }
    }

    /// One reconcile round over both sides, answering whether either moved
    /// and whether both were quiet ([`quiet`]).
    ///
    /// A side's divergences are itemized right after its own merge. One
    /// between the endpoints is the store's: it keeps the agreed body and
    /// parks the diverging binding (pimdir SYNC §9) for the side's merge.
    fn reconcile_pass(
        &mut self,
        run: AccountRun<'_>,
        collection: &str,
        push: bool,
        report: &mut SyncOutput,
    ) -> Result<(bool, bool)> {
        let (store_dir, dry_run) = (run.work_dir, run.dry_run);
        let Self {
            left,
            right,
            left_store,
            right_store,
            blobs,
            ..
        } = self;
        let (left_report, left_resolved) = reconcile_side(
            collection, left, left_store, blobs, store_dir, push, dry_run, report,
        )?;
        let (right_report, right_resolved) = reconcile_side(
            collection,
            right,
            right_store,
            blobs,
            store_dir,
            push,
            dry_run,
            report,
        )?;

        let moved = moved(&left_report) || moved(&right_report);
        let quiet =
            quiet(&left_report) && quiet(&right_report) && left_resolved + right_resolved == 0;
        Ok((moved, quiet))
    }
}

/// One side of a reconcile round: its merge, its probes and its report,
/// with how many conflicts it resolved.
///
/// Only a pulling round's events name what the remote did, a pushing round's
/// echoing this run's own writes. A side nothing writes is listed by the
/// pulling round alone: a pushing one would only list it again.
#[allow(clippy::too_many_arguments)]
fn reconcile_side(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    store_dir: &Path,
    push: bool,
    dry_run: bool,
    report: &mut SyncOutput,
) -> Result<(PimdirSyncReport, usize)> {
    if push && !dry_run && !ctx.writable() {
        let resolved = resolve_conflicts(collection, ctx, store, blobs, store_dir, dry_run)?;
        return Ok((PimdirSyncReport::default(), resolved));
    }

    let push = push && !dry_run && ctx.writable();
    let before = if push {
        None
    } else {
        Some(flag_snapshot(store, collection, &ctx.name)?)
    };

    let (side, _) = sync_side_rebuilding(collection, ctx, store, blobs, push, None)?;
    let resolved = resolve_conflicts(collection, ctx, store, blobs, store_dir, dry_run)?;

    let display = display_name(&ctx.namespace, collection);

    match &before {
        Some(before) => itemize_pulled(
            &side.events,
            before,
            store,
            collection,
            display,
            &ctx.name,
            report,
        )?,
        None => itemize_conflicted(&side.events, store, collection, &ctx.name, report)?,
    }

    Ok((side, resolved))
}

/// Whether a side's sync changed anything, used to detect convergence.
pub(super) fn moved(report: &PimdirSyncReport) -> bool {
    report.pulled > 0 || report.pushed > 0
}
