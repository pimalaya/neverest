//! # Hydration
//!
//! Bodies fetched into the store: phase 2 of a local run through one
//! work-stealing pool, and the bodies a pair copies or mirrors.

use crate::sync::driver::*;

/// The connections a Microsoft Graph source opens unless its `pool-size`
/// says otherwise.
///
/// Graph serves four requests at once a mailbox, the inner requests of a
/// `$batch` counting among them, so three leave room for another client of
/// the mailbox.
const GRAPH_CONNECTIONS: usize = 3;

/// How many connections a side may open, each one worker of the run.
///
/// IMAP takes `connections`, Microsoft Graph its `pool-size` (else
/// [`GRAPH_CONNECTIONS`]) capped by `connections`. Others keep one: Gmail
/// beside a batch was never measured, Google meters the whole account.
pub(super) fn connection_budget(config: &SourceConfig, connections: usize) -> usize {
    let connections = connections.max(1);
    match &config.backend {
        SourceBackendConfig::Imap(_) => connections,
        SourceBackendConfig::Msgraph(_)
        | SourceBackendConfig::MsgraphContacts(_)
        | SourceBackendConfig::MsgraphCalendar(_) => config
            .pool_size()
            .unwrap_or(GRAPH_CONNECTIONS)
            .clamp(1, connections),
        SourceBackendConfig::Gmail(_) => 1,
        _ => 1,
    }
}

/// A collection's spine result: its name and the bodies to hydrate.
pub(super) type CollectionPlan = (String, Vec<HydrateTarget>);

/// A body to hydrate, with what orders its download.
#[derive(Clone, Debug)]
pub(super) struct HydrateTarget {
    pub(super) handle: PimdirHandle,
    /// The size its local mail summary carries, 0 when unknown.
    pub(super) size: u64,
    /// Its date, the mail summary's RFC 3339 instant in UTC.
    pub(super) date: Option<String>,
}

/// One hydrate batch: a collection and the bodies fetched together.
///
/// Each body carries the size the store records (0 for none), which a backend
/// reading by size plans its reads with.
#[derive(Debug, PartialEq)]
pub(super) struct HydrateBatch {
    pub(super) collection: String,
    pub(super) bodies: Vec<(PimdirHandle, u64)>,
}

impl HydrateBatch {
    /// The batch of `targets`, read from `collection`.
    pub(super) fn new<'t>(
        collection: &str,
        targets: impl IntoIterator<Item = &'t HydrateTarget>,
    ) -> Self {
        Self {
            collection: collection.to_string(),
            bodies: targets
                .into_iter()
                .map(|target| (target.handle.clone(), target.size))
                .collect(),
        }
    }
}

/// The bodies the servers answered gone this run, by collection.
///
/// No later batch of the run asks for them again. The store keeps each until
/// the next listing drops it (pimdir SYNC §5), so a gone body is never
/// fetched in a loop, nor dropped on the word of a body read alone.
#[derive(Default)]
pub(super) struct GoneBodies(Mutex<HashMap<String, HashSet<String>>>);

impl GoneBodies {
    fn record<'h>(&self, collection: &str, handles: impl IntoIterator<Item = &'h PimdirHandle>) {
        let mut gone = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let gone = gone.entry(collection.to_string()).or_default();
        gone.extend(handles.into_iter().map(|handle| handle.0.clone()));
    }

    fn holds(&self, collection: &str, handle: &PimdirHandle) -> bool {
        let gone = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        gone.get(collection)
            .is_some_and(|gone| gone.contains(&handle.0))
    }

    /// `targets` but the ones gone from `collection`.
    pub(super) fn keep(&self, collection: &str, targets: Vec<HydrateTarget>) -> Vec<HydrateTarget> {
        let gone = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(gone) = gone.get(collection) else {
            return targets;
        };
        targets
            .into_iter()
            .filter(|target| !gone.contains(&target.handle.0))
            .collect()
    }
}

/// What the connections landing downloaded bodies share in a phase.
pub(super) struct Landing<'a> {
    pub(super) kind: Kind,
    pub(super) blobs: &'a PimdirBlobStage,
    /// Each collection's gate, which its spine holds but while it waits on
    /// the server (phase 1); none once every spine is over (phase 2).
    pub(super) gates: Option<&'a HashMap<&'a str, Mutex<()>>>,
    /// Serialises the writes of the connections' store handles.
    pub(super) applying: &'a Mutex<()>,
    /// The source's collections, when one handle names one item across
    /// them ([`Client::global_handles`]): Gmail's labels.
    pub(super) shared: Option<&'a [String]>,
    pub(super) gone: &'a GoneBodies,
    /// Called for each body of a batch once it is settled: downloaded,
    /// served from the store, gone, or owed nowhere any more.
    pub(super) on_body: Option<&'a (dyn Fn() + Sync)>,
}

impl Landing<'_> {
    /// Downloads `batch` and raises each body to `Full` where it is owed,
    /// answering how many bodies came over the network.
    ///
    /// Account-wide handles are downloaded once, or served from a collection
    /// holding them. A collection's gate is taken before `applying`, one
    /// collection at a time, so no applier ever holds two gates.
    pub(super) fn land(
        &self,
        ctx: &mut SourceCtx,
        store: &mut PimdirSourceStore,
        batch: &HydrateBatch,
    ) -> Result<usize> {
        let collection = batch.collection.as_str();
        let asked: Vec<PimdirHandle> = batch
            .bodies
            .iter()
            .map(|(handle, _)| handle)
            .filter(|handle| !self.gone.holds(collection, handle))
            .cloned()
            .collect();
        let placed = match self.shared {
            Some(collections) => Placed::read(store, collections, &asked).unwrap_or_else(|err| {
                warn!("cannot read where the bodies of {collection} are owed ({err:#}), landing them there alone");
                Placed::alone(collection, &asked)
            }),
            None => Placed::alone(collection, &asked),
        };

        let fetch = placed.to_fetch(&asked);
        let sizes: HashMap<&str, u64> = batch
            .bodies
            .iter()
            .filter(|(_, size)| *size > 0)
            .map(|(handle, size)| (handle.as_str(), *size))
            .collect();
        let sized = |id: &str| sizes.get(id).copied();
        let (mut items, gone) = match fetch.is_empty() {
            true => (Vec::new(), Vec::new()),
            false => {
                let hydrated = hydrate_batch(
                    self.kind,
                    ctx.pool.primary(),
                    wire_name(&ctx.namespace, collection),
                    &fetch,
                    &sized,
                    self.blobs,
                    self.on_body,
                )?;
                ctx.pool.downloaded(body_bytes(&hydrated.items));
                (hydrated.items, hydrated.gone)
            }
        };
        let fetched = items.len();

        for (handle, hash) in placed.stored {
            match stored_item(self.kind, self.blobs.blobs(), handle.clone(), hash) {
                Ok(Some(item)) => items.push(item),
                Ok(None) => debug!("the stored body of {} is gone", handle.as_str()),
                Err(err) => warn!("cannot serve {} from the store: {err:#}", handle.as_str()),
            }
        }
        if let Some(on_body) = self.on_body {
            (fetched..batch.bodies.len()).for_each(|_| on_body());
        }

        if !gone.is_empty() {
            let gone: HashSet<&str> = gone.iter().map(PimdirHandle::as_str).collect();
            for (place, handles) in &placed.owed {
                self.gone.record(
                    place,
                    handles
                        .iter()
                        .filter(|handle| gone.contains(handle.as_str())),
                );
            }
            info!(
                "{} bodies of {} gone from the server since they were listed, left to the next listing",
                gone.len(),
                display_name(&ctx.namespace, collection),
            );
        }

        let items: HashMap<String, PimdirFetchedItem> = items
            .into_iter()
            .map(|item| (item.handle.0.clone(), item))
            .collect();
        for (place, handles) in placed.owed {
            let landed: Vec<PimdirFetchedItem> = handles
                .iter()
                .filter_map(|handle| items.get(&handle.0).cloned())
                .collect();
            if landed.is_empty() {
                continue;
            }
            let _gate = self
                .gates
                .and_then(|gates| gates.get(place.as_str()))
                .map(|gate| gate.lock().unwrap_or_else(PoisonError::into_inner));
            let _applying = self.applying.lock().unwrap_or_else(PoisonError::into_inner);
            let options = ctx.upgrade_options();
            let fallback = PimRemote::new(&mut ctx.pool, self.blobs.clone(), ctx.namespace.clone());
            match apply_batch(store, fallback, &place, landed, options) {
                Ok(upgraded) => ctx.noted.note_events(&place, &upgraded.events),
                Err(err) => warn!("{place} write error: {err:#}"),
            }
        }

        Ok(fetched)
    }
}

/// Where the bodies of a batch are owed.
struct Placed {
    /// Each collection holding some of the handles without a body, with
    /// those handles, in the run's order.
    owed: Vec<(String, Vec<PimdirHandle>)>,
    /// The body a collection already holds for a handle another owes.
    stored: Vec<(PimdirHandle, PimdirHash)>,
}

impl Placed {
    /// The handles owed by their own collection alone.
    fn alone(collection: &str, handles: &[PimdirHandle]) -> Self {
        Self {
            owed: vec![(collection.to_string(), handles.to_vec())],
            stored: Vec::new(),
        }
    }

    /// Where `handles` are owed across `collections`, on a source whose
    /// handles are account-wide.
    fn read(
        store: &PimdirSourceStore,
        collections: &[String],
        handles: &[PimdirHandle],
    ) -> Result<Self> {
        let mut owed = Vec::new();
        let mut owing: HashSet<String> = HashSet::new();
        let mut held: HashMap<String, PimdirHash> = HashMap::new();
        if handles.is_empty() {
            return Ok(Self {
                owed,
                stored: Vec::new(),
            });
        }
        for collection in collections {
            let id = PimdirCollectionId(collection.clone());
            let loaded = store
                .load(&id, &PimdirLoadSelection::Handles(handles.to_vec()))
                .with_context(|| format!("Load {collection}"))?;
            let mut its = Vec::new();
            for placement in loaded.placements {
                if placement.status == PimdirStatus::Tombstone {
                    continue;
                }
                match placement.object {
                    Some(hash) => {
                        held.entry(placement.handle.0).or_insert(hash);
                    }
                    None => {
                        owing.insert(placement.handle.0.clone());
                        its.push(placement.handle);
                    }
                }
            }
            if !its.is_empty() {
                owed.push((collection.clone(), its));
            }
        }
        let stored = handles
            .iter()
            .filter(|handle| owing.contains(&handle.0))
            .filter_map(|handle| Some((handle.clone(), held.remove(&handle.0)?)))
            .collect();
        Ok(Self { owed, stored })
    }

    /// The handles of `asked` to download: owed somewhere, and held in
    /// no collection already, in the order asked.
    fn to_fetch(&self, asked: &[PimdirHandle]) -> Vec<PimdirHandle> {
        let owed: HashSet<&str> = self
            .owed
            .iter()
            .flat_map(|(_, handles)| handles.iter().map(PimdirHandle::as_str))
            .collect();
        let stored: HashSet<&str> = self
            .stored
            .iter()
            .map(|(handle, _)| handle.as_str())
            .collect();
        asked
            .iter()
            .filter(|handle| owed.contains(handle.as_str()) && !stored.contains(handle.as_str()))
            .cloned()
            .collect()
    }
}

/// Phase 2, hydrating every body through one global work-stealing pool.
///
/// A batched fetch stays within one collection, so bodies are chunked per
/// collection and queued across the account; each batch is raised to `Full`
/// once fetched, so an interrupted run keeps it.
#[allow(clippy::too_many_arguments)]
pub(super) fn phase2_hydrate(
    source: &str,
    plans: Vec<CollectionPlan>,
    pacing: &Pacing,
    ctxs: &mut [SourceCtx],
    stores: &mut [PimdirSourceStore],
    blobs: &PimdirBlobStage,
    shared: Option<&[String]>,
    gone: &GoneBodies,
    progress: Progress<'_>,
) -> Result<()> {
    let batches = hydrate_batches(plans, pacing.order, pacing.prime, shared.is_some());
    let total_bodies: usize = batches.iter().map(|batch| batch.bodies.len()).sum();
    if total_bodies == 0 {
        return Ok(());
    }

    let kind = ctxs
        .first_mut()
        .map(|ctx| resolve_kind(&mut ctx.pool))
        .unwrap_or(Kind::Mail);

    let done = AtomicUsize::new(0);
    let fetched = AtomicUsize::new(0);
    let gate = PercentGate::default();
    let download_event = |done| SyncEvent::DownloadProgressed {
        source: source.to_owned(),
        done,
        total: total_bodies,
    };
    progress.emit(|| download_event(0));
    let on_body = || {
        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
        if gate.passes(n, total_bodies) {
            progress.emit(|| download_event(n));
        }
    };
    let applying = Mutex::new(());
    let landing = Landing {
        kind,
        blobs,
        gates: None,
        applying: &applying,
        shared,
        gone,
        on_body: Some(&on_body),
    };

    let mut workers: Vec<(&mut SourceCtx, &mut PimdirSourceStore)> =
        ctxs.iter_mut().zip(stores.iter_mut()).collect();
    hydrate_pool(&mut workers, batches, |(ctx, store), batch| {
        let n = landing.land(ctx, store, &batch)?;
        fetched.fetch_add(n, Ordering::Relaxed);
        Ok(())
    })?;

    Ok(())
}

/// Chunks each collection's bodies into batches, queued in `order`.
///
/// Each collection's `prime` newest bodies come first, then `order` across
/// the account, ties to the collection the run takes first. Under `shared`
/// (Gmail) a handle is planned once, under the first collection listing it.
pub(super) fn hydrate_batches(
    plans: Vec<CollectionPlan>,
    order: DownloadOrder,
    prime: usize,
    shared: bool,
) -> Vec<HydrateBatch> {
    let mut names = Vec::with_capacity(plans.len());
    let mut tiered: Vec<(u8, usize, HydrateTarget)> = Vec::new();
    let mut planned: HashMap<String, usize> = HashMap::new();
    for (rank, (collection, mut targets)) in plans.into_iter().enumerate() {
        names.push(collection);
        targets.sort_by(|a, b| DownloadOrder::Newest.compare(a, b));
        for (at, target) in targets.into_iter().enumerate() {
            let tier = u8::from(at >= prime);
            if shared {
                if let Some(&first) = planned.get(&target.handle.0) {
                    let held = &mut tiered[first].0;
                    *held = (*held).min(tier);
                    continue;
                }
                planned.insert(target.handle.0.clone(), tiered.len());
            }
            tiered.push((tier, rank, target));
        }
    }
    drop(planned);

    tiered.sort_by(|(tier_a, rank_a, a), (tier_b, rank_b, b)| {
        rank_a
            .cmp(rank_b)
            .then(tier_a.cmp(tier_b))
            .then(order.compare(a, b))
    });
    let mut batches: Vec<(u8, usize, HydrateTarget, HydrateBatch)> = Vec::new();
    for group in tiered.chunk_by(|a, b| (a.0, a.1) == (b.0, b.1)) {
        let (tier, rank) = (group[0].0, group[0].1);
        for chunk in group.chunks(BATCH_SIZE) {
            let batch = HydrateBatch::new(&names[rank], chunk.iter().map(|(_, _, t)| t));
            batches.push((tier, rank, chunk[0].2.clone(), batch));
        }
    }
    batches.sort_by(|(tier_a, rank_a, a, _), (tier_b, rank_b, b, _)| {
        tier_a
            .cmp(tier_b)
            .then(order.compare(a, b))
            .then(rank_a.cmp(rank_b))
    });
    batches.into_iter().map(|(_, _, _, batch)| batch).collect()
}

/// Runs `batches` over one worker per connection, each landing its batch
/// before taking the next.
///
/// A landing error (a failed download) stops the pool and fails the run,
/// the batches landed before it kept; `land` only warns on a failed write,
/// its batch left for the next run.
pub(super) fn hydrate_pool<W: Send>(
    workers: &mut [W],
    batches: Vec<HydrateBatch>,
    land: impl Fn(&mut W, HydrateBatch) -> Result<()> + Sync,
) -> Result<()> {
    let queue: SegQueue<HydrateBatch> = SegQueue::new();
    for batch in batches {
        queue.push(batch);
    }

    let failure: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let stop = AtomicBool::new(false);

    let queue_ref = &queue;
    let failure_ref = &failure;
    let stop_ref = &stop;
    let land_ref = &land;

    thread::scope(|scope| {
        for worker in workers.iter_mut() {
            scope.spawn(move || {
                while !stop_ref.load(Ordering::Relaxed) && !crate::sync::stopping() {
                    let Some(batch) = queue_ref.pop() else {
                        break;
                    };
                    if let Err(err) = land_ref(worker, batch) {
                        *failure_ref.lock().unwrap() = Some(err);
                        stop_ref.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            });
        }
    });

    match failure.into_inner().unwrap() {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Raises one fetched batch to `Full`, serving its bodies from the batch.
///
/// A handle the batch lacks is fetched through `fallback`. A divergence whose
/// body the batch brings is settled under `options` (pimdir SYNC §6).
pub(super) fn apply_batch<R: PimdirRemote<Error = anyhow::Error>>(
    store: &mut PimdirSourceStore,
    fallback: R,
    collection: &str,
    items: Vec<PimdirFetchedItem>,
    options: PimdirUpgradeOptions,
) -> Result<PimdirUpgradeReport> {
    let handles: Vec<PimdirHandle> = items.iter().map(|item| item.handle.clone()).collect();
    if handles.is_empty() {
        return Ok(PimdirUpgradeReport::default());
    }
    let mut remote = CachedFetchRemote::new(collection, items, fallback);
    let upgrade =
        PimdirUpgrade::new(collection.to_string(), handles, PimdirTier::Full).options(options);
    run_verb(store, &mut remote, upgrade).with_context(|| format!("Apply bodies {collection}"))
}

/// Hydrates the bodies of one-sided items the other side may receive.
///
/// The hub then holds the body and its next projection stages the copy.
pub(super) fn hydrate_copies(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    progress: &CollectionProgress,
) -> Result<usize> {
    let targets = hydration_targets(
        left_store,
        collection,
        HydrationSide {
            name: &left.name,
            creates: left.perms.item.create,
            updates: left.perms.item.update,
            decides: left.authority == Authority::Endpoint,
        },
        HydrationSide {
            name: &right.name,
            creates: right.perms.item.create,
            updates: right.perms.item.update,
            decides: right.authority == Authority::Endpoint,
        },
    )
    .with_context(|| format!("Hydration targets {collection}"))?;
    if targets.is_empty() {
        return Ok(0);
    }

    let mut left_handles = Vec::new();
    let mut right_handles = Vec::new();
    for (source, handle) in &targets {
        if *source == left.name {
            left_handles.push(handle.clone());
        } else {
            right_handles.push(handle.clone());
        }
    }
    let total = targets.len();
    let done = AtomicUsize::new(0);
    let tick = || progress.tick(done.fetch_add(1, Ordering::Relaxed) + 1, total);

    for (ctx, store, handles) in [
        (left, left_store, left_handles),
        (right, right_store, right_handles),
    ] {
        if handles.is_empty() {
            continue;
        }
        let options = ctx.upgrade_options();
        let mut remote = PimRemote::with_progress(
            &mut ctx.pool,
            blobs.clone(),
            ctx.namespace.clone(),
            &tick,
            HashMap::new(),
        );
        let upgrade =
            PimdirUpgrade::new(collection.to_string(), handles, PimdirTier::Full).options(options);
        let upgraded = run_verb(store, &mut remote, upgrade)
            .with_context(|| format!("Hydrate bodies for {} {collection}", ctx.name))?;
        ctx.noted.note_events(collection, &upgraded.events);
    }
    Ok(total)
}

/// Hydrates every not-yet-`Full` placement of both sides.
///
/// The store then mirrors every body, and the shared object store dedups, so
/// the second side's upgrade links an already-stored body by link id without a
/// fetch.
#[allow(clippy::too_many_arguments)]
pub(super) fn hydrate_full_collection(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    progress: &CollectionProgress,
) -> Result<usize> {
    let mut raised = 0;
    for (ctx, store) in [(left, left_store), (right, right_store)] {
        let targets: Vec<PimdirHandle> = load_side(store, collection)
            .with_context(|| format!("Load {} {collection}", ctx.name))?
            .into_iter()
            .filter(|p| {
                p.status != PimdirStatus::Tombstone
                    && (p.level < PimdirLevel::Full || p.awaits_base())
            })
            .map(|p| p.handle)
            .collect();
        if targets.is_empty() {
            continue;
        }
        let total = targets.len();
        let done = AtomicUsize::new(0);
        let tick = || progress.tick(done.fetch_add(1, Ordering::Relaxed) + 1, total);
        let options = ctx.upgrade_options();
        let mut remote = PimRemote::with_progress(
            &mut ctx.pool,
            blobs.clone(),
            ctx.namespace.clone(),
            &tick,
            HashMap::new(),
        );
        let upgrade =
            PimdirUpgrade::new(collection.to_string(), targets, PimdirTier::Full).options(options);
        let upgraded = run_verb(store, &mut remote, upgrade)
            .with_context(|| format!("Hydrate all bodies {} {collection}", ctx.name))?;
        ctx.noted.note_events(collection, &upgraded.events);
        raised += total;
    }
    Ok(raised)
}
