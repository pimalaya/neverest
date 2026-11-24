//! # Local run
//!
//! The one-source sync, its phases run account-wide, and the run that only
//! performs what frontends queued, opening each endpoint's session.

use io_pimdir::collection::PimdirCheckpoint;

use crate::sync::driver::*;

/// A side's permissions as io-pimdir's push rights for the kind it syncs.
///
/// Only mail carries flags on the wire: a flag on a card or calendar item is
/// the store's alone, so the right is withheld rather than pushing a change
/// the backend rejects on every run.
pub(super) fn push_rights(perms: &SourcePermissions, media_type: &str) -> PimdirPushRights {
    PimdirPushRights {
        flags: perms.flag.update && media_type == "message/rfc822",
        content: perms.item.update,
        add: perms.item.create,
        remove: perms.item.delete,
    }
}

/// Where a collection's role puts it in the run, what a reader opens first.
///
/// Inbox (or a default calendar or address book), sent, drafts, the rest by
/// name, the all-mail archive repeating them, junk, then trash.
fn role_rank(role: Option<&str>) -> u8 {
    match role {
        Some("inbox" | "default") => 0,
        Some("sent") => 1,
        Some("drafts") => 2,
        Some("all") => 4,
        Some("junk") => 5,
        Some("trash") => 6,
        _ => 3,
    }
}

/// The collection ids `kept`, in the order the run takes them: by role
/// (see [`role_rank`]), then by name, ASCII case aside, then by id.
pub(super) fn run_order(
    kept: &BTreeSet<String>,
    names: &BTreeMap<String, String>,
    roles: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut ordered: Vec<String> = kept.iter().cloned().collect();
    ordered.sort_by_cached_key(|id| {
        let role = role_rank(roles.get(id).map(String::as_str));
        let name = names.get(id).unwrap_or(id).to_ascii_lowercase();
        (role, name, id.clone())
    });
    ordered
}

/// The local, one-source sync, run as two account-wide phases.
///
/// Both phases run account-wide so the pool never idles at a collection edge,
/// the hydrate plans read after the spine so nothing landed is fetched again.
/// With no target, `one-way` discards what was staged locally.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_local(
    account_name: &str,
    account_config: &AccountConfig,
    account: &Account,
    mode: &AccountMode,
    source_name: &str,
    source_config: SourceConfig,
    collection_filter: Option<CollectionFilter>,
    scope: &PimdirScope,
    work_dir: &Path,
    dry_run: bool,
    accept_absences: bool,
    connections: usize,
    pacing: Pacing,
    declare_only: bool,
    progress: Progress<'_>,
    state: &mut AccountState,
    report: &mut SyncOutput,
) -> Result<()> {
    let source = source_name.to_string();
    let source_filter = source_config.collection().filter.clone();

    let workers = if declare_only {
        1
    } else {
        connection_budget(&source_config, connections)
    };
    let authority = if mode.one_way {
        Authority::Endpoint
    } else {
        Authority::Shared
    };

    let source_account = account.get(source_name)?;
    let mut ctxs = open_source_contexts(
        &source,
        source_name,
        &source_config,
        source_account,
        authority,
        workers,
        scope,
        dry_run,
        accept_absences,
    )?;
    let mut stores: Vec<PimdirSourceStore> = (0..workers)
        .map(|_| open_store(work_dir, &source, account_name))
        .collect::<Result<_>>()?;
    let blobs = stores[0].blob_stage();
    progress.emit(|| SyncEvent::EndpointOpened {
        endpoint: source.clone(),
    });

    let raw = ctxs[0].pool.primary().media_type();
    let kind = Kind::from_media_type(raw)
        .with_context(|| format!("This build cannot sync items of type {raw}"))?;
    let media_type = kind.media_type();

    if !dry_run {
        let (first, _) = ctxs.split_first_mut().expect("at least one connection");
        let renames = drain_collection_intents(&mut [&mut *first], &mut stores[0], false, report);
        if !declare_only {
            drain_submits(
                account_config,
                account,
                &mut [SendSide::open(&mut *first)],
                &mut stores[0],
                &blobs,
                work_dir,
                &renames,
                report,
            );
            drain_invitations(&mut [first], &mut stores[0], work_dir, report);
        }
    }

    let listed = ctxs[0]
        .pool
        .primary()
        .list_collections(false)
        .context("List collections error")?;
    let collections: BTreeMap<String, String> = listed
        .iter()
        .map(|collection| (collection.id.clone(), collection.name.clone()))
        .collect();
    progress.emit(|| SyncEvent::EndpointListed {
        endpoint: source.clone(),
        collections: collections.len(),
    });

    let filter = if declare_only {
        CollectionFilter::All
    } else {
        collection_filter.unwrap_or(source_filter)
    };
    let kept = filter_collections(&collections, &filter);

    let stated = stated_roles(
        || ctxs[0].pool.primary().lookup_roles(),
        source_name,
        &listed,
        state,
    );
    let roles = stated.clone().unwrap_or_else(|| {
        listed
            .iter()
            .filter_map(|c| Some((c.id.clone(), c.role.clone()?)))
            .collect()
    });
    let kept = run_order(&kept, &collections, &roles);
    let filtered: Vec<String> = kept.iter().map(|id| hub_id(source_name, id)).collect();

    let listing = Listing {
        listed: &listed,
        names: &collections,
        kept: &kept,
        roles: stated.as_ref(),
    };
    let declaring = Declaring {
        dir: work_dir,
        account: account_name,
        config: account_config,
        mode,
    };
    let fresh = record_listing(
        &stores[0],
        &declaring,
        state,
        source_name,
        media_type,
        &listing,
        dry_run,
    )?;
    for collection in fresh {
        let hunk = CollectionHunk::Fetch {
            side: source_name.to_owned(),
            collection,
        };
        report.note_collection_hunk(hunk, None);
    }

    if declare_only {
        return Ok(());
    }

    let pacing = Pacing {
        prime: if dry_run { 0 } else { pacing.prime },
        ..pacing
    };
    let listed_log = (pacing.bodies == Bodies::Listed).then(Listed::default);
    let shared = ctxs[0].pool.primary().global_handles();
    let gone = GoneBodies::default();

    let quiet = quiet_collections(&mut ctxs[0], &mut stores[0], &filtered)?;
    let spining: Vec<String> = filtered
        .iter()
        .filter(|collection| !quiet.contains(collection))
        .cloned()
        .collect();
    for collection in &quiet {
        report_coverage(&stores[0], collection, &ctxs[0], report);
    }

    let mut spined = phase1_spine(
        source_name,
        &spining,
        &mut ctxs,
        &mut stores,
        &blobs,
        work_dir,
        dry_run,
        &pacing,
        listed_log.as_ref(),
        shared,
        &gone,
        progress,
        report,
    )?;
    spined.extend(quiet);

    if dry_run {
        return Ok(());
    }

    let plans = hydrate_plans(&stores[0], source_name, &spined, listed_log.as_ref(), &gone)?;
    let hydrated = phase2_hydrate(
        source_name,
        plans,
        &pacing,
        &mut ctxs,
        &mut stores,
        &blobs,
        shared.then_some(&filtered[..]),
        &gone,
        progress,
    );
    for ctx in &mut ctxs {
        itemize_noted(ctx, report);
    }
    hydrated
}

/// The collections of `filtered` phase 1 leaves out: settled under the
/// run's scope, and named by no change the source states before listing.
///
/// A delta of such a collection would derive nothing but its checkpoint
/// (pimdir SYNC §5), moved here where the source stands, all in one write.
pub(super) fn quiet_collections(
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    filtered: &[String],
) -> Result<Vec<String>> {
    if ctx.dry_run || !ctx.pool.primary().names_quiet_collections() {
        return Ok(Vec::new());
    }
    let settled = store
        .settled_collections()
        .map_err(pimdir_error)
        .context("Read the settled collections")?;

    let mut named = Vec::new();
    for (id, coverage) in settled {
        if coverage.scope != ctx.scope || !filtered.contains(&id.0) {
            continue;
        }
        let loaded = store
            .load(&id, &PimdirLoadSelection::Handles(Vec::new()))
            .map_err(pimdir_error)
            .with_context(|| format!("Load the checkpoint of {}", id.0))?;
        let Some(checkpoint) = loaded.checkpoint else {
            continue;
        };
        let Ok(checkpoint) = String::from_utf8(checkpoint.0) else {
            continue;
        };
        named.push((display_name(&ctx.namespace, &id.0).to_owned(), checkpoint));
    }
    if named.is_empty() {
        return Ok(Vec::new());
    }

    let namespace = ctx.namespace.clone();
    let reader: &PimdirSourceStore = store;
    let held = |name: &str, handles: &[String]| -> Result<HashSet<String>> {
        let collection = PimdirCollectionId(hub_id(&namespace, name));
        let handles = handles.iter().map(|h| PimdirHandle(h.clone())).collect();
        let loaded = reader
            .load(&collection, &PimdirLoadSelection::Handles(handles))
            .map_err(pimdir_error)
            .with_context(|| format!("Load the bindings of {}", collection.0))?;
        Ok(loaded.placements.into_iter().map(|p| p.handle.0).collect())
    };
    let quiet = ctx.pool.primary().quiet_collections(&named, held)?;
    if quiet.is_empty() {
        return Ok(Vec::new());
    }

    let writes: Vec<PimdirWriteOp> = quiet
        .iter()
        .filter(|quiet| !named.contains(quiet))
        .map(|(name, next)| PimdirWriteOp::SetCheckpoint {
            collection: PimdirCollectionId(hub_id(&namespace, name)),
            checkpoint: PimdirCheckpoint(next.clone().into_bytes()),
        })
        .collect();
    if !writes.is_empty() {
        store
            .write(writes)
            .map_err(pimdir_error)
            .context("Move the checkpoints of the quiet collections")?;
    }
    debug!("{} collection(s) named no change, left out", quiet.len());

    Ok(quiet
        .into_iter()
        .map(|(name, _)| hub_id(&namespace, &name))
        .collect())
}

/// What a declaration reads: the store, the account and its mode.
pub(super) struct Declaring<'a> {
    pub(super) dir: &'a Path,
    pub(super) account: &'a str,
    pub(super) config: &'a AccountConfig,
    pub(super) mode: &'a AccountMode,
}

impl Declaring<'_> {
    /// Declares the account's sources again (see [`declare`]) from `state`,
    /// a failure logged: the declaration before the drain still stands.
    pub(super) fn again(&self, state: &AccountState) {
        if let Err(err) = declare(self.dir, self.account, self.config, self.mode, state) {
            warn!("cannot declare what the sources hold after a listing: {err:#}");
        }
    }

    /// Records `source` as syncing `collections` from their listing on.
    ///
    /// The declaration that follows then holds before their first checkpoint
    /// ([`register_syncing`]); a failure is logged, the first item sync
    /// records it.
    pub(super) fn register(&self, source: &str, collections: &[String]) {
        match register_syncing(self.dir, source, collections) {
            Ok(0) => {}
            Ok(n) => debug!("{source} registered as syncing {n} new collection(s)"),
            Err(err) => warn!("cannot record {source} as syncing what it listed: {err:#}"),
        }
    }
}

/// What one source's listing answered, as a run keeps it.
pub(super) struct Listing<'a> {
    /// Every collection listed.
    pub(super) listed: &'a [Collection],
    /// Their names, by id.
    pub(super) names: &'a BTreeMap<String, String>,
    /// The ids the run keeps, in run order.
    pub(super) kept: &'a [String],
    /// The roles the source states, by id, `None` when a lookup failed.
    pub(super) roles: Option<&'a BTreeMap<String, String>>,
}

/// Records what a local listing discovered before any item of it moves,
/// answering the kept collections the store did not hold yet.
///
/// Each kept collection's kind, name and role and the taskless calendars,
/// then the source registered and declared again, so a calendar met first is
/// gated from its listing on (pimdir STORAGE §15.6), dry runs aside.
pub(super) fn record_listing(
    store: &PimdirSourceStore,
    declaring: &Declaring<'_>,
    state: &mut AccountState,
    source: &str,
    media_type: &str,
    listing: &Listing<'_>,
    dry_run: bool,
) -> Result<Vec<String>> {
    record_taskless(state, source, &taskless_ids(source, listing.listed));

    let held: HashSet<String> = store
        .list_collections()
        .context("List the store's collections")?
        .into_iter()
        .map(|collection| collection.id)
        .collect();
    let mut fresh = Vec::new();
    for id in listing.kept {
        let collection = &hub_id(source, id);
        if !held.contains(collection) {
            fresh.push(id.clone());
        }
        store
            .ensure_collection(collection, media_type)
            .with_context(|| format!("Declare kind for {collection}"))?;
        declare_name(store, collection, id, &[listing.names]);
        if let Some(roles) = listing.roles {
            declare_role(store, collection, roles.get(id).map(String::as_str));
        }
    }

    if !dry_run {
        let kept: Vec<String> = listing.kept.iter().map(|id| hub_id(source, id)).collect();
        declaring.register(source, &kept);
        declaring.again(state);
    }
    Ok(fresh)
}

/// Performs what frontends queued for one source and its targets, syncing
/// nothing: collection intents, sends and calendar intents.
///
/// An endpoint opens only when an intent or a native send (Graph) needs its
/// session, so an `smtp` send leaves while the source's server is down. One
/// that does not open is reported unreached; the others still run theirs.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_intents(
    account_name: &str,
    account_config: &AccountConfig,
    account: &Account,
    mode: &AccountMode,
    endpoints: &HashMap<String, SourceConfig>,
    source_name: &str,
    scope: &PimdirScope,
    work_dir: &Path,
    report: &mut SyncOutput,
) -> Result<()> {
    let names: Vec<&str> = std::iter::once(source_name)
        .chain(mode.targets.iter().map(String::as_str))
        .collect();
    let mut store = open_store(work_dir, source_name, account_name)?;
    let blobs = store.blob_stage();
    let wanted = wanted_sides(&store, account_config, &names, source_name);

    let mut opened: Vec<SourceCtx> = Vec::new();
    for &name in names.iter().filter(|name| wanted.contains(**name)) {
        let authority = match (mode.one_way, name == source_name) {
            (false, _) => Authority::Shared,
            (true, true) => Authority::Endpoint,
            (true, false) => Authority::Store,
        };
        let ctxs = account.get(name).and_then(|side| {
            open_source_contexts(
                name,
                source_name,
                &endpoints[name],
                side,
                authority,
                1,
                scope,
                false,
                false,
            )
        });
        match ctxs {
            Ok(ctxs) => opened.extend(ctxs),
            Err(err) => note_source_failure(name, err, report),
        }
    }

    let renames = drain_collection_intents(
        &mut opened.iter_mut().collect::<Vec<_>>(),
        &mut store,
        !mode.targets.is_empty(),
        report,
    );

    let mut open = opened.iter_mut().peekable();
    let mut sides: Vec<SendSide<'_>> = Vec::with_capacity(names.len());
    for &name in &names {
        match open.next_if(|ctx| ctx.name == name) {
            Some(ctx) => sides.push(SendSide::open(ctx)),
            None => sides.push(SendSide::unopened(name)),
        }
    }
    drain_submits(
        account_config,
        account,
        &mut sides,
        &mut store,
        &blobs,
        work_dir,
        &renames,
        report,
    );
    drop(sides);

    drain_invitations(
        &mut opened.iter_mut().collect::<Vec<_>>(),
        &mut store,
        work_dir,
        report,
    );
    Ok(())
}

/// The endpoints among `names` whose session a queued intent needs, the
/// drains picking their performer as they do.
///
/// An intent needs the endpoint it names, else its namespace's first (a
/// producer predating pimdir STORAGE §15.6); an unnamed calendar intent may
/// need any. A send needs the sending endpoint only when it sends natively.
pub(super) fn wanted_sides(
    store: &PimdirStore,
    account_config: &AccountConfig,
    names: &[&str],
    namespace: &str,
) -> BTreeSet<String> {
    let prefix = format!("{namespace}/");
    let mut wanted = BTreeSet::new();
    let mut want = |named: Option<String>, collection: &str, any: bool| match named {
        Some(name) if names.contains(&name.as_str()) => {
            wanted.insert(name);
        }
        Some(_) => {}
        None if collection.starts_with(&prefix) && any => {
            wanted.extend(names.iter().map(|name| name.to_string()));
        }
        None if collection.starts_with(&prefix) => {
            wanted.insert(names[0].to_owned());
        }
        None => {}
    };

    match collection::pending(store) {
        Ok(intents) => {
            for intent in intents {
                want(intent.source(), &intent.collection, false);
            }
        }
        Err(err) => warn!("cannot read the queued collection intents: {err:#}"),
    }
    match invitation::pending(store) {
        Ok(intents) => {
            for intent in intents {
                want(intent.source(), &intent.collection, true);
            }
        }
        Err(err) => warn!("cannot read the queued calendar intents: {err:#}"),
    }

    let sides: Vec<SendSide<'_>> = names.iter().map(|name| SendSide::unopened(name)).collect();
    let sender = sending_source(account_config, &sides);
    let native = sender.as_ref().is_some_and(|sender| {
        account_config.sources().is_ok_and(|sources| {
            sources
                .get(sender)
                .is_some_and(|source| source.smtp.is_none() && source.sends_natively())
        })
    });
    if let Some(sender) = sender.filter(|_| native) {
        let queued = submit::pending(store).is_ok_and(|intents| {
            intents
                .iter()
                .any(|intent| intent.source().is_none_or(|source| source == sender))
        });
        if queued {
            wanted.insert(sender);
        }
    }

    wanted
}

/// Records an endpoint the run could not sync or open: a failed `*` scan,
/// and the endpoint unreached when the failure says so.
pub(super) fn note_source_failure(source_name: &str, err: anyhow::Error, report: &mut SyncOutput) {
    warn!("source {source_name} sync error: {err:#}");
    if let Some(unreached) = err.downcast_ref::<Unreached>() {
        let endpoint = unreached.endpoint.clone();
        report.note_unreached(endpoint, Failure::of(&err));
    }
    let scan = CollectionHunk::Scan {
        side: source_name.to_owned(),
        collection: String::from("*"),
    };
    report.note_collection_hunk(scan, Some(err));
}

/// Opens `count` single-connection [`SourceCtx`]s with overlapping handshakes.
#[allow(clippy::too_many_arguments)]
fn open_source_contexts(
    name: &str,
    namespace: &str,
    config: &SourceConfig,
    account: SourceAccount,
    authority: Authority,
    count: usize,
    scope: &PimdirScope,
    dry_run: bool,
    accept_absences: bool,
) -> Result<Vec<SourceCtx>> {
    let perms = config.permissions();
    let capped = config.collection().capped;
    let opened: Vec<Result<Pool>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..count)
            .map(|_| {
                let account = account.clone();
                scope.spawn(move || Pool::open(account, 1))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("connection open thread"))
            .collect()
    });
    let mut ctxs = Vec::with_capacity(count);
    for pool in opened {
        ctxs.push(SourceCtx {
            name: name.to_string(),
            namespace: namespace.to_string(),
            perms,
            authority,
            pool: pool.with_context(|| Unreached::new(name, "Open connection"))?,
            refused: Vec::new(),
            rejected: Vec::new(),
            scope: scope.clone(),
            dry_run,
            accept_absences,
            capped,
            noted: Noted::default(),
        });
    }
    Ok(ctxs)
}
