//! # Spine phase
//!
//! Phase 1 of a local run: each collection's listing, its metadata and flags,
//! synced in parallel, each over its own connection.

use crate::sync::driver::*;

/// Phase 1: spines each collection in parallel, returning those it finished.
///
/// Reads run through WAL, writes serialise on the store's lock; each report is
/// absorbed whole, arms and all, or the run would not name what it left. Under
/// `--prime` all list their newest page first ([`Work`]).
#[allow(clippy::too_many_arguments)]
pub(super) fn phase1_spine(
    source: &str,
    filtered: &[String],
    ctxs: &mut [SourceCtx],
    stores: &mut [PimdirSourceStore],
    blobs: &PimdirBlobStage,
    store_dir: &Path,
    dry_run: bool,
    pacing: &Pacing,
    listed: Option<&Listed>,
    shared: bool,
    gone: &GoneBodies,
    progress: Progress<'_>,
    report: &mut SyncOutput,
) -> Result<Vec<String>> {
    let total = filtered.len();
    let prime = pacing.prime;
    let work = Work::new(pacing.order, total, prime > 0);
    let spined: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    let carried: Mutex<HashMap<usize, HashSet<String>>> = Mutex::new(HashMap::new());
    let merged: Mutex<SyncOutput> = Mutex::new(SyncOutput::default());
    let scanned = AtomicUsize::new(0);
    let pushing: Mutex<()> = Mutex::new(());
    let settled = match (dry_run, stores.first()) {
        (false, Some(store)) => Settled::read(store, store_dir)?,
        _ => Settled::default(),
    };
    let settled_ref = &settled;
    let scan_event = |done| SyncEvent::ScanProgressed {
        source: source.to_owned(),
        done,
        total,
    };
    progress.emit(|| scan_event(0));

    let feeding = !dry_run && ctxs.len() > 1;
    let gates: HashMap<&str, Mutex<()>> = filtered
        .iter()
        .map(|collection| (collection.as_str(), Mutex::new(())))
        .collect();
    let applying: Mutex<()> = Mutex::new(());
    let kind = ctxs
        .first_mut()
        .map(|ctx| resolve_kind(&mut ctx.pool))
        .unwrap_or(Kind::Mail);
    let tally: Mutex<Tally> = Mutex::new(Tally::default());
    let gate = PercentGate::default();
    let download_event = |done, total| SyncEvent::DownloadProgressed {
        source: source.to_owned(),
        done,
        total,
    };
    let on_body = || {
        let mut tally = tally.lock().unwrap_or_else(PoisonError::into_inner);
        tally.settled += 1;
        if let Some((before, left)) = tally.window {
            let done = tally.settled - before;
            if done <= left && gate.passes(done, left) {
                progress.emit(|| download_event(done, left));
            }
        }
    };
    let landing = Landing {
        kind,
        blobs,
        gates: Some(&gates),
        applying: &applying,
        shared: shared.then_some(filtered),
        gone,
        on_body: Some(&on_body),
    };
    let tally_ref = &tally;

    let work_ref = &work;
    let spined_ref = &spined;
    let carried_ref = &carried;
    let merged_ref = &merged;
    let scanned_ref = &scanned;
    let pushing_ref = &pushing;
    let gates_ref = &gates;
    let landing_ref = &landing;

    thread::scope(|scope| {
        for (ctx, store) in ctxs.iter_mut().zip(stores.iter_mut()) {
            scope.spawn(move || {
                while let Some(job) = work_ref.next() {
                    let (index, pass) = match job {
                        Job::Spine { index, pass } => (index, pass),
                        Job::Hydrate { batch, primed } => {
                            match landing_ref.land(ctx, store, &batch) {
                                Ok(_) => {}
                                Err(err) => {
                                    warn!("{} body download error: {err:#}", batch.collection)
                                }
                            }
                            work_ref.downloaded(primed);
                            continue;
                        }
                    };

                    let collection = &filtered[index];
                    let owed = carried_ref
                        .lock()
                        .unwrap()
                        .remove(&index)
                        .unwrap_or_default();
                    let mut pager = PageFeed {
                        feed: (feeding && pass != Pass::First).then_some(work_ref),
                        rank: index,
                        resumed: pass == Pass::Whole,
                        pause: pass == Pass::First,
                        listed,
                        ..PageFeed::hold(&gates_ref[collection.as_str()])
                    };
                    let spine = collection_spine(
                        collection,
                        ctx,
                        store,
                        blobs,
                        store_dir,
                        pushing_ref,
                        settled_ref,
                        dry_run,
                        &mut pager,
                        owed,
                    );
                    drop(pager);
                    let paused = match spine {
                        Ok(Spined {
                            report,
                            paused,
                            owed,
                        }) => {
                            merged_ref.lock().unwrap().absorb(report);
                            if paused {
                                carried_ref.lock().unwrap().insert(index, owed);
                            } else {
                                spined_ref.lock().unwrap().push(index);
                            }
                            paused
                        }
                        Err(err) => {
                            warn!("{collection} scan error: {err:#}");
                            let display = display_name(&ctx.namespace, collection).to_string();
                            let scan = CollectionHunk::Scan {
                                side: ctx.name.clone(),
                                collection: display,
                            };
                            merged_ref
                                .lock()
                                .unwrap()
                                .note_collection_hunk(scan, Some(err));
                            report_coverage(
                                store,
                                collection,
                                ctx,
                                &mut merged_ref.lock().unwrap(),
                            );
                            false
                        }
                    };
                    if pass == Pass::First {
                        prime_bodies(work_ref, store, collection, index, prime, feeding, listed);
                    }
                    work_ref.spined(pass, index, paused);
                    if !paused {
                        let n = scanned_ref.fetch_add(1, Ordering::Relaxed) + 1;
                        progress.emit(|| scan_event(n));
                        if n == total && feeding {
                            let mut tally =
                                tally_ref.lock().unwrap_or_else(PoisonError::into_inner);
                            let left = work_ref.fed().saturating_sub(tally.settled);
                            if left > 0 {
                                tally.window = Some((tally.settled, left));
                                progress.emit(|| download_event(0, left));
                            }
                        }
                    }
                }
            });
        }
    });

    let mut spined = spined.into_inner().unwrap();
    spined.sort_unstable();
    report.absorb(merged.into_inner().unwrap());
    Ok(spined
        .into_iter()
        .map(|index| filtered[index].clone())
        .collect())
}

/// Queues the bodies a collection owes once the first pass over it ended.
///
/// Its `prime` newest by date come first whatever the order, being what a
/// reader opens; then, connections to spare, the rest beside the pages'
/// bodies. What is not queued waits for phase 2.
fn prime_bodies(
    work: &Work,
    store: &PimdirSourceStore,
    collection: &str,
    rank: usize,
    prime: usize,
    feeding: bool,
    listed: Option<&Listed>,
) {
    let id = PimdirCollectionId(collection.to_string());
    let mut owed = match store.load(&id, &PimdirLoadSelection::All) {
        Ok(loaded) => bodiless(loaded.placements),
        Err(err) => {
            warn!("{collection} load error: {err}");
            return;
        }
    };
    if let Some(listed) = listed {
        owed = listed.keep(collection, owed);
    }
    owed.sort_by(|a, b| DownloadOrder::Newest.compare(a, b));
    let rest = owed.split_off(prime.min(owed.len()));
    work.feed(collection, rank, owed, true);
    if feeding {
        work.feed(collection, rank, rest, false);
    }
}

/// The handles each collection's listing landed this run, kept under
/// [`Bodies::Listed`], whose run downloads those bodies alone.
#[derive(Default)]
pub(super) struct Listed(Mutex<HashMap<String, HashSet<String>>>);

impl Listed {
    pub(super) fn record(&self, collection: &str, landed: &[PimdirHandle]) {
        if landed.is_empty() {
            return;
        }
        let mut listed = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        listed
            .entry(collection.to_string())
            .or_default()
            .extend(landed.iter().map(|handle| handle.0.clone()));
    }

    /// The targets of `collection` this run listed.
    pub(super) fn keep(&self, collection: &str, targets: Vec<HydrateTarget>) -> Vec<HydrateTarget> {
        let listed = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(handles) = listed.get(collection) else {
            return Vec::new();
        };
        targets
            .into_iter()
            .filter(|target| handles.contains(&target.handle.0))
            .collect()
    }
}

/// Which pass a phase-1 spine runs in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pass {
    /// Priming off: the collection listed to its end.
    Whole,
    /// Priming: the collection listed up to its first page, a round left
    /// open there, a delta to its end.
    First,
    /// Priming over: a collection the first pass paused, listed to its end.
    Fill,
}

/// The queued bodies phase 1 settled, and the download its scan's end opens.
#[derive(Default)]
struct Tally {
    settled: usize,
    /// Those settled before the scan ended and those left then, once the
    /// last collection is listed with bodies still to download.
    window: Option<(usize, usize)>,
}

/// What a phase-1 connection does next.
#[derive(Debug, PartialEq)]
pub(super) enum Job {
    /// Spine the collection at `index` of the run's order.
    Spine { index: usize, pass: Pass },
    /// Download a batch, `primed` when it holds a collection's newest.
    Hydrate { batch: HydrateBatch, primed: bool },
}

/// What a queued batch is ranked by: its first body, then its collection's
/// place in the run.
#[derive(Debug)]
struct BatchKey {
    first: HydrateTarget,
    rank: usize,
}

/// Phase 1's work, handed to the connections in turn.
///
/// A connection takes, in order: a primed batch; a collection the first pass
/// has not reached; a paused one to fill; else the next batch in the download
/// order. Only batches are held, so memory is the budget times a batch.
pub(super) struct Work {
    order: DownloadOrder,
    state: Mutex<WorkState>,
    ready: Condvar,
}

struct WorkState {
    /// The collections the first pass has not taken, in the run's order.
    first: VecDeque<usize>,
    /// The pass they are taken in: [`Pass::First`] under priming, else
    /// [`Pass::Whole`].
    pass: Pass,
    /// The collections the first pass paused, waiting for it to end.
    fill: BTreeSet<usize>,
    /// The spines under way.
    spining: usize,
    /// The spines under way taken from `first`.
    priming: usize,
    /// The batches of primed bodies, and how many are downloading.
    primed: Vec<(BatchKey, HydrateBatch)>,
    downloading: usize,
    /// The batches of every other body.
    batches: Vec<(BatchKey, HydrateBatch)>,
    /// How many bodies were queued, primed or not.
    fed: usize,
}

impl Work {
    /// The work of `collections` spines, primed or listed whole.
    pub(super) fn new(order: DownloadOrder, collections: usize, prime: bool) -> Self {
        Self {
            order,
            state: Mutex::new(WorkState {
                first: (0..collections).collect(),
                pass: if prime { Pass::First } else { Pass::Whole },
                fill: BTreeSet::new(),
                spining: 0,
                priming: 0,
                primed: Vec::new(),
                downloading: 0,
                batches: Vec::new(),
                fed: 0,
            }),
            ready: Condvar::new(),
        }
    }

    /// Queues `collection`'s bodiless targets, in batches of [`BATCH_SIZE`]
    /// sorted in the download order.
    pub(super) fn feed(
        &self,
        collection: &str,
        rank: usize,
        mut targets: Vec<HydrateTarget>,
        primed: bool,
    ) {
        if targets.is_empty() {
            return;
        }
        targets.sort_by(|a, b| self.order.compare(a, b));
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.fed += targets.len();
        let queue = if primed {
            &mut state.primed
        } else {
            &mut state.batches
        };
        for chunk in targets.chunks(BATCH_SIZE) {
            let key = BatchKey {
                first: chunk[0].clone(),
                rank,
            };
            queue.push((key, HydrateBatch::new(collection, chunk)));
        }
        drop(state);
        self.ready.notify_all();
    }

    /// How many bodies were queued so far.
    pub(super) fn fed(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .fed
    }

    /// A spine ended, whether it succeeded or not, `paused` when the first
    /// pass left its round open.
    pub(super) fn spined(&self, pass: Pass, index: usize, paused: bool) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.spining = state.spining.saturating_sub(1);
        if pass != Pass::Fill {
            state.priming = state.priming.saturating_sub(1);
        }
        if paused {
            state.fill.insert(index);
        }
        drop(state);
        self.ready.notify_all();
    }

    /// A batch was downloaded, applied or not.
    pub(super) fn downloaded(&self, primed: bool) {
        if !primed {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.downloading = state.downloading.saturating_sub(1);
        drop(state);
        self.ready.notify_all();
    }

    /// The next job, waiting while one may still come.
    ///
    /// `None` once every spine and primed download ended and nothing is queued.
    /// The first pass ends with its last spine and primed body, so no
    /// collection lists a second page before every one listed its first.
    pub(super) fn next(&self) -> Option<Job> {
        if crate::sync::stopping() {
            return None;
        }

        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(batch) = self.take(&mut state.primed) {
                state.downloading += 1;
                return Some(Job::Hydrate {
                    batch,
                    primed: true,
                });
            }
            if let Some(index) = state.first.pop_front() {
                state.spining += 1;
                state.priming += 1;
                let pass = state.pass;
                return Some(Job::Spine { index, pass });
            }
            if state.priming == 0
                && state.downloading == 0
                && let Some(index) = state.fill.pop_first()
            {
                state.spining += 1;
                return Some(Job::Spine {
                    index,
                    pass: Pass::Fill,
                });
            }
            if let Some(batch) = self.take(&mut state.batches) {
                return Some(Job::Hydrate {
                    batch,
                    primed: false,
                });
            }
            if state.spining == 0 && state.downloading == 0 {
                return None;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Takes the batch of `queue` that comes first.
    fn take(&self, queue: &mut Vec<(BatchKey, HydrateBatch)>) -> Option<HydrateBatch> {
        let first = queue
            .iter()
            .enumerate()
            .min_by(|(_, (a, _)), (_, (b, _))| {
                self.order
                    .compare(&a.first, &b.first)
                    .then(a.rank.cmp(&b.rank))
            })
            .map(|(index, _)| index)?;
        Some(queue.remove(first).1)
    }
}

/// The [`Pager`] of a collection's spine in phase 1.
///
/// It holds the collection's gate for the whole spine but the server waits, so
/// a body lands only at rest, never between a page's load and its write, which
/// would write back the placement without the body.
pub(super) struct PageFeed<'a> {
    pub(super) gate: &'a Mutex<()>,
    pub(super) held: Option<MutexGuard<'a, ()>>,
    pub(super) feed: Option<&'a Work>,
    /// The collection's place in the run, which breaks the feed's ties.
    pub(super) rank: usize,
    /// Whether a resumed round's earlier bodiless members feed before its page.
    pub(super) resumed: bool,
    /// Whether the listing stops before the second page of a round.
    pub(super) pause: bool,
    pub(super) listed: Option<&'a Listed>,
}

impl<'a> PageFeed<'a> {
    /// A pager feeding nothing, its gate held.
    pub(super) fn hold(gate: &'a Mutex<()>) -> Self {
        Self {
            gate,
            held: Some(gate.lock().unwrap_or_else(PoisonError::into_inner)),
            feed: None,
            rank: 0,
            resumed: true,
            pause: false,
            listed: None,
        }
    }
}

impl Pager for PageFeed<'_> {
    fn at_rest(
        &mut self,
        store: &mut PimdirSourceStore,
        collection: &PimdirCollectionId,
        landed: Vec<PimdirHandle>,
        resumed: bool,
        next: Option<&PimdirListing>,
    ) -> Rest {
        if let Some(listed) = self.listed {
            listed.record(collection.as_str(), &landed);
        }
        if self.pause
            && matches!(
                next,
                Some(PimdirListing::Round {
                    cursor: Some(_),
                    ..
                })
            )
        {
            return Rest::Pause;
        }
        let Some(feed) = self.feed else {
            return Rest::Go;
        };
        let selection = match (resumed && self.resumed, landed.is_empty()) {
            (true, _) => PimdirLoadSelection::All,
            (false, true) => return Rest::Go,
            (false, false) => PimdirLoadSelection::Handles(landed),
        };
        match store.load(collection, &selection) {
            Ok(loaded) => {
                let mut owed = bodiless(loaded.placements);
                if let Some(listed) = self.listed {
                    owed = listed.keep(collection.as_str(), owed);
                }
                feed.feed(collection.as_str(), self.rank, owed, false);
            }
            Err(err) => warn!("{} page load error: {err}", collection.as_str()),
        }
        Rest::Go
    }

    fn listing(&mut self) {
        self.held = None;
    }

    fn listed(&mut self) {
        self.held = Some(self.gate.lock().unwrap_or_else(PoisonError::into_inner));
    }
}

/// The placements still owed a body, or a base body under an edit (pimdir
/// SYNC §5), with what orders their download.
///
/// A create, pending or delivered, is owed none: its provisional handle
/// names no member a server could answer for (pimdir SYNC §2), and the page
/// that lands it brings the handle its body is read under.
fn bodiless(placements: impl IntoIterator<Item = PimdirPlacement>) -> Vec<HydrateTarget> {
    placements
        .into_iter()
        .filter(|placement| {
            placement.status != PimdirStatus::Tombstone
                && !placement.handle.is_provisional()
                && (placement.object.is_none() || placement.awaits_base())
        })
        .map(|placement| HydrateTarget {
            size: summary_size(&placement.summary).unwrap_or(0) as u64,
            date: summary_date(&placement.summary),
            handle: placement.handle,
        })
        .collect()
}

/// Each spined collection's bodies still to download, for phase 2, only
/// those the run listed under [`Bodies::Listed`], none the run found gone.
pub(super) fn hydrate_plans(
    store: &PimdirStore,
    source: &str,
    collections: &[String],
    listed: Option<&Listed>,
    gone: &GoneBodies,
) -> Result<Vec<CollectionPlan>> {
    collections
        .iter()
        .map(|collection| {
            let view = projection_view(store, collection, source)
                .with_context(|| format!("Project {source} {collection}"))?;
            let owed = gone.keep(collection, bodiless(view));
            let owed = match listed {
                Some(listed) => listed.keep(collection, owed),
                None => owed,
            };
            Ok((collection.clone(), owed))
        })
        .collect()
}

/// A collection's spine: its report, and whether the pager paused it.
struct Spined {
    report: SyncOutput,
    /// Whether the listing stopped at rest, its round left open.
    paused: bool,
    /// When paused, what the spine resuming it counts as owed.
    ///
    /// The bodies owed before this one, and the members it pulled, whose
    /// bodies may arrive in between.
    owed: HashSet<String>,
}

/// Reconciles one collection's spine, its bodies left to hydration.
///
/// A dry run stops after itemizing; a paused spine leaves its round open for
/// the one `owed` resumes. One collection pushes at a time (`pushing`), or a
/// move's halves would both read the create pending (pimdir SYNC §5).
#[allow(clippy::too_many_arguments)]
fn collection_spine(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    store_dir: &Path,
    pushing: &Mutex<()>,
    settled: &Settled,
    dry_run: bool,
    pager: &mut dyn Pager,
    owed: HashSet<String>,
) -> Result<Spined> {
    let mut report = SyncOutput::default();

    let loaded =
        load_side(store, collection).with_context(|| format!("Load {} {collection}", ctx.name))?;
    let mut owed = owed;
    owed.extend(
        loaded
            .iter()
            .filter(|placement| {
                placement.status != PimdirStatus::Tombstone && placement.object.is_none()
            })
            .map(|placement| placement.handle.0.clone()),
    );
    let before: HashMap<String, PimdirFlags> = loaded
        .into_iter()
        .map(|placement| (placement.handle.0, placement.flags))
        .collect();

    let (pull, paused) = sync_side_rebuilding(collection, ctx, store, blobs, false, Some(pager))?;
    let display = display_name(&ctx.namespace, collection);
    let resolved = resolve_conflicts(collection, ctx, store, blobs, store_dir, dry_run)?;
    let pulled_quiet = quiet(&pull) && resolved == 0;
    if !pulled_quiet {
        settled.changed();
    }
    itemize_pulled(
        &pull.events,
        &before,
        store,
        collection,
        display,
        &ctx.name,
        &mut report,
    )?;
    if paused {
        owed.extend(pull.events.iter().filter_map(|event| match event {
            PimdirSyncEvent::Added(handle) | PimdirSyncEvent::ContentChanged(handle) => {
                Some(handle.0.clone())
            }
            _ => None,
        }));
        itemize_noted(ctx, &mut report);
        return Ok(Spined {
            report,
            paused,
            owed,
        });
    }
    itemize_fetches(
        collection,
        display,
        store,
        &ctx.name,
        &pull.events,
        &owed,
        &mut report,
    )?;
    itemize_single(collection, store, ctx, &mut report)?;
    if dry_run {
        itemize_noted(ctx, &mut report);
        return Ok(Spined {
            report,
            paused,
            owed: HashSet::new(),
        });
    }

    let push = pushing.lock().unwrap_or_else(PoisonError::into_inner);
    let mut done = settled.owes_no_push(collection, ctx, store, store_dir, pulled_quiet)?;
    for _ in 0..=MAX_EXTRA_PASSES {
        if done {
            break;
        }
        let (pass, _) = sync_side_rebuilding(collection, ctx, store, blobs, ctx.writable(), None)?;
        if !quiet(&pass) {
            settled.changed();
        }
        if !moved(&pass) {
            done = true;
            break;
        }
    }
    drop(push);
    if !done {
        report.unsettled.push(collection.to_owned());
    }
    report_coverage(store, collection, ctx, &mut report);
    itemize_noted(ctx, &mut report);
    itemize_refused(&ctx.name, mem::take(&mut ctx.refused), &mut report);
    itemize_rejected(&ctx.name, mem::take(&mut ctx.rejected), &mut report);
    Ok(Spined {
        report,
        paused,
        owed: HashSet::new(),
    })
}
