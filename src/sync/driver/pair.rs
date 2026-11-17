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

/// One source against every target the account names, each pairing on its own.
///
/// Both endpoints bind the source's namespace, which is what makes them meet.
/// Targets run in turn rather than together, no target influencing another
/// under `one-way`, so the source is enumerated once per target.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_targets(
    account_name: &str,
    account_config: &AccountConfig,
    account: &Account,
    mode: &AccountMode,
    endpoints: &HashMap<String, SourceConfig>,
    source_name: &str,
    source_config: SourceConfig,
    collection_filter: Option<CollectionFilter>,
    scope: &PimdirScope,
    work_dir: &Path,
    dry_run: bool,
    accept_absences: bool,
    connections: usize,
    progress: Progress<'_>,
    state: &mut AccountState,
    report: &mut SyncOutput,
) -> Result<()> {
    let relay = mode.streams(endpoints);
    let mut taskless = PairTaskless::new(state, source_name);

    for target_name in &mode.targets {
        run_pair(
            account_name,
            account_config,
            account,
            mode,
            source_name,
            source_name.to_string(),
            source_config.clone(),
            target_name.clone(),
            endpoints[target_name].clone(),
            collection_filter.clone(),
            scope,
            relay,
            work_dir,
            dry_run,
            accept_absences,
            connections,
            progress,
            state,
            &mut taskless,
            report,
        )?;
    }

    record_taskless(state, source_name, &taskless.listed);
    Ok(())
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
#[allow(clippy::too_many_arguments)]
fn run_pair(
    account_name: &str,
    account_config: &AccountConfig,
    account: &Account,
    mode: &AccountMode,
    namespace: &str,
    left_name: String,
    left_config: SourceConfig,
    right_name: String,
    right_config: SourceConfig,
    collection_filter: Option<CollectionFilter>,
    scope: &PimdirScope,
    relay: bool,
    work_dir: &Path,
    dry_run: bool,
    accept_absences: bool,
    connections: usize,
    progress: Progress<'_>,
    state: &mut AccountState,
    taskless: &mut PairTaskless,
    report: &mut SyncOutput,
) -> Result<()> {
    let left_filter = left_config.collection().filter.clone();

    let mut left_store = open_store(work_dir, &left_name, account_name)?;
    let mut right_store = open_store(work_dir, &right_name, account_name)?;
    let blobs = left_store.blob_stage();

    let hydrate_full = mode.retain;

    let left_budget = connection_budget(&left_config, connections);
    let right_budget = connection_budget(&right_config, connections);

    let (left_authority, right_authority) = if mode.one_way {
        (Authority::Endpoint, Authority::Store)
    } else {
        (Authority::Shared, Authority::Shared)
    };

    let left_account = account.get(&left_name)?;
    let right_account = account.get(&right_name)?;

    let mut left = SourceCtx {
        name: left_name.clone(),
        namespace: namespace.to_string(),
        perms: left_config.permissions(),
        authority: left_authority,
        pool: Pool::open(left_account, left_budget)
            .with_context(|| Unreached::new(&left_name, format!("Open source {left_name}")))?,
        refused: Vec::new(),
        rejected: Vec::new(),
        scope: scope.clone(),
        dry_run,
        accept_absences,
        capped: left_config.collection().capped,
        noted: Noted::default(),
    };
    let mut right = SourceCtx {
        name: right_name.clone(),
        namespace: namespace.to_string(),
        perms: right_config.permissions(),
        authority: right_authority,
        pool: Pool::open(right_account, right_budget)
            .with_context(|| Unreached::new(&right_name, format!("Open target {right_name}")))?,
        refused: Vec::new(),
        rejected: Vec::new(),
        scope: scope.clone(),
        dry_run,
        accept_absences,
        capped: right_config.collection().capped,
        noted: Noted::default(),
    };
    for endpoint in [&left_name, &right_name] {
        progress.emit(|| SyncEvent::Opened {
            endpoint: endpoint.clone(),
        });
    }

    let kind = check_kinds(&mut left, &mut right)?;
    let media_type = kind.media_type();

    if !dry_run {
        drain_collection_intents(&mut [&mut left, &mut right], &mut left_store, true, report);
        drain_submits(
            account_config,
            account,
            &mut [SendSide::open(&mut left), SendSide::open(&mut right)],
            &mut left_store,
            &blobs,
            work_dir,
            report,
        );
        drain_invitations(
            &mut [&mut left, &mut right],
            &mut left_store,
            work_dir,
            report,
        );
    }

    let left_listed = left
        .pool
        .primary()
        .list_collections(false)
        .context("List collections error")?;
    let right_listed = right
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
        progress.emit(|| SyncEvent::Listed {
            endpoint: endpoint.clone(),
            collections: listed.len(),
        });
    }

    let filter = collection_filter.unwrap_or(left_filter);
    let left_filtered = filter_collections(&left_collections, &filter);
    let right_filtered = filter_collections(&right_collections, &filter);

    let held = held_by_both(
        &left_store,
        namespace,
        left_filtered.symmetric_difference(&right_filtered),
        &left_name,
        &right_name,
    )?;
    let collection_hunks = diff_collections(
        &left_filtered,
        &right_filtered,
        &held,
        (&left_name, &left.perms.collection),
        (&right_name, &right.perms.collection),
    );
    for hunk in collection_hunks {
        let error = if dry_run {
            None
        } else {
            apply_collection_hunk(&hunk, &mut left, &mut right).err()
        };
        if !dry_run
            && error.is_none()
            && let CollectionHunk::Delete { collection, .. } = &hunk
        {
            let id = hub_id(namespace, collection);
            if let Err(err) = left_store.delete_collection(&id) {
                warn!("cannot drop {id} from the store: {err}");
            }
        }
        report.collection_patch.push(PatchEntry::new(hunk, error));
    }

    let mut present_left = left_filtered.clone();
    let mut present_right = right_filtered.clone();
    for entry in &report.collection_patch {
        if entry.error.is_some() {
            continue;
        }
        if let CollectionHunk::Create { side, collection } = &entry.hunk {
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
        let known = left_store.ensure_collection(&collection, media_type);
        declare_name(
            &left_store,
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
        dir: work_dir,
        account: account_name,
        config: account_config,
        mode,
    };
    record_pair_listing(
        &declaring,
        state,
        namespace,
        taskless,
        [&left_listed, &right_listed],
        &syncing,
        dry_run,
    );

    let total = common.len();
    for (index, name) in common.iter().enumerate() {
        if crate::sync::stopping() {
            break;
        }

        let collection = &hub_id(namespace, name);
        progress.emit(|| SyncEvent::CollectionStarted {
            source: left_name.clone(),
            target: right_name.clone(),
            collection: name.clone(),
            position: index + 1,
            total,
        });
        let finished = |failure: Option<Failure>| {
            let (code, error, sources) = Failure::row(failure);
            SyncEvent::CollectionFinished {
                source: left_name.clone(),
                target: right_name.clone(),
                collection: name.clone(),
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

        if let Err(err) = sync_collection(
            collection,
            media_type,
            &mut left,
            &mut right,
            &mut left_store,
            &mut right_store,
            &blobs,
            work_dir,
            dry_run,
            relay,
            &ticks,
            report,
        ) {
            warn!("{name} sync error: {err:#}");
            progress.emit(|| finished(Some(Failure::of(&err))));
            let hunk = CollectionHunk::Sync {
                source: left_name.clone(),
                target: right_name.clone(),
                collection: name.clone(),
            };
            report
                .collection_patch
                .push(PatchEntry::new(hunk, Some(err)));
            continue;
        }

        let mut error = None;
        if hydrate_full
            && !dry_run
            && let Err(err) = hydrate_full_collection(
                collection,
                &mut left,
                &mut right,
                &mut left_store,
                &mut right_store,
                &blobs,
                &ticks,
            )
        {
            warn!("{name} full hydration error: {err:#}");
            error = Some(Failure::of(&err));
            report.failures.push(StepFailure::new(
                FailedStep::Bodies,
                Some(name.clone()),
                &err,
            ));
        }
        itemize_noted(&mut left, report);
        itemize_noted(&mut right, report);

        progress.emit(|| finished(error));
    }

    Ok(())
}

/// Reconciles one collection until quiescent.
///
/// Its own report is folded into the account's by [`SyncOutput::absorb`], the
/// one-source path's fold, so no arm is dropped on one path; it is folded on
/// failure too, what it reported before stopping having still happened.
#[allow(clippy::too_many_arguments)]
fn sync_collection(
    collection: &str,
    media_type: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    store_dir: &Path,
    dry_run: bool,
    relay: bool,
    progress: &CollectionProgress,
    account: &mut SyncOutput,
) -> Result<()> {
    left_store
        .ensure_collection(collection, media_type)
        .with_context(|| format!("Declare kind for {collection}"))?;

    let mut report = SyncOutput::default();
    let outcome = sync_collection_into(
        collection,
        left,
        right,
        left_store,
        right_store,
        blobs,
        store_dir,
        dry_run,
        relay,
        progress,
        &mut report,
    );

    report_coverage(left_store, collection, left, &mut report);
    report_coverage(right_store, collection, right, &mut report);
    itemize_noted(left, &mut report);
    itemize_noted(right, &mut report);

    account.absorb(report);

    outcome
}

/// The body of [`sync_collection`], filling that collection's own report.
///
/// The opening round only pulls, so its events are the remotes' own rather
/// than the echo of this run's writes, and no push clears a projection before
/// `itemize` reads it.
#[allow(clippy::too_many_arguments)]
fn sync_collection_into(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    store_dir: &Path,
    dry_run: bool,
    relay: bool,
    progress: &CollectionProgress,
    report: &mut SyncOutput,
) -> Result<()> {
    reconcile_pass(
        collection,
        left,
        right,
        left_store,
        right_store,
        blobs,
        store_dir,
        false,
        dry_run,
        report,
    )?;
    propagate(
        collection,
        left,
        right,
        left_store,
        right_store,
        blobs,
        relay,
        progress,
        report,
    )?;

    itemize(collection, left_store, left, right, report)?;
    if dry_run {
        return Ok(());
    }

    let mut settled = false;
    for _ in 0..=MAX_EXTRA_PASSES {
        let progressed = reconcile_pass(
            collection,
            left,
            right,
            left_store,
            right_store,
            blobs,
            store_dir,
            true,
            dry_run,
            report,
        )?;
        let propagated = propagate(
            collection,
            left,
            right,
            left_store,
            right_store,
            blobs,
            relay,
            progress,
            report,
        )?;
        if !progressed && propagated == 0 {
            settled = true;
            break;
        }
    }
    if !settled {
        report.unsettled.push(collection.to_owned());
    }

    itemize_refused(&left.name, mem::take(&mut left.refused), report);
    itemize_refused(&right.name, mem::take(&mut right.refused), report);
    itemize_rejected(&left.name, mem::take(&mut left.rejected), report);
    itemize_rejected(&right.name, mem::take(&mut right.rejected), report);

    Ok(())
}

/// Propagates the collection's cross-side copies, returning how many moved.
///
/// Retaining hydrates the body into the store for the projection to push;
/// relaying streams it server-to-server and keeps only the spine.
#[allow(clippy::too_many_arguments)]
fn propagate(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    relay: bool,
    progress: &CollectionProgress,
    report: &mut SyncOutput,
) -> Result<usize> {
    if relay {
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

/// One reconcile round over both sides, answering whether either moved.
///
/// A side's divergences are itemized right after its own merge. A divergence
/// between the endpoints themselves is the store's: it keeps the agreed body
/// and parks the diverging binding (pimdir SYNC §9) for the side's merge.
#[allow(clippy::too_many_arguments)]
fn reconcile_pass(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    store_dir: &Path,
    push: bool,
    dry_run: bool,
    report: &mut SyncOutput,
) -> Result<bool> {
    let left_report = reconcile_side(
        collection, left, left_store, blobs, store_dir, push, dry_run, report,
    )?;
    let right_report = reconcile_side(
        collection,
        right,
        right_store,
        blobs,
        store_dir,
        push,
        dry_run,
        report,
    )?;

    Ok(moved(&left_report) || moved(&right_report))
}

/// One side of a reconcile round: its merge, its probes and its report.
///
/// Only a pulling round's events name what the remote did. A pushing round's
/// `FlagsChanged` and `Vanished` also echo this run's own writes, so itemizing
/// them would report the run's work back as the remote's.
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
) -> Result<PimdirSyncReport> {
    let push = push && !dry_run && ctx.writable();
    let before = if push {
        None
    } else {
        Some(flag_snapshot(store, collection, &ctx.name)?)
    };

    let (side, _) = sync_side_rebuilding(collection, ctx, store, blobs, push, None)?;
    resolve_conflicts(collection, ctx, store, blobs, store_dir, dry_run)?;

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
        None => itemize_conflicted(&side.events, store, collection, display, &ctx.name, report)?,
    }

    Ok(side)
}

/// Whether a side's sync changed anything, used to detect convergence.
pub(super) fn moved(report: &PimdirSyncReport) -> bool {
    report.pulled > 0 || report.pushed > 0
}
