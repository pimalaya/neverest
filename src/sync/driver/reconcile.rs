//! # Side sync
//!
//! One side's run of the engine, rekeying a renumbered collection, and the
//! conflicts it leaves, resolved or merged.

use crate::sync::driver::*;

/// Runs one side's sync, rekeying the collection first when its listing
/// found the source renumbered every member (pimdir SYNC §4, §8).
///
/// The rekey carries every placement onto the new handles by link id, then
/// the sync runs again; a failed or parked rekey writes nothing, so the next
/// run meets the renumbering again. Also answers whether `pager` paused it.
pub(super) fn sync_side_rebuilding(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    push: bool,
    mut pager: Option<&mut dyn Pager>,
) -> Result<(PimdirSyncReport, bool)> {
    let first = pager.as_mut().map(|pager| &mut **pager as &mut dyn Pager);
    let synced = match sync_side(collection, ctx, store, blobs, push, first) {
        Err(err) if err.downcast_ref::<Renumbered>().is_some() => {
            info!("handles of {collection} were renumbered, rebuilding the collection");
            let rekey = rebuild_collection(collection, ctx, store, blobs)?;
            if let Some(parked) = rekey.parked {
                warn!(
                    "rebuilding {collection} would drop {} of {} items, parked",
                    parked.absent, parked.bound
                );
                ctx.noted.park(collection, parked, true);
                return Ok((PimdirSyncReport::default(), false));
            }
            let generation = store
                .generation(collection)
                .with_context(|| format!("Read the generation of {collection}"))?
                .context("Rekey completed without a write")?;
            info!(
                "{collection} rebuilt under generation {generation}: {} carried, {} pulled, {} pending dropped",
                rekey.rekeyed, rekey.pulled, rekey.dropped
            );
            sync_side(collection, ctx, store, blobs, push, pager)
        }
        synced => synced,
    }?;
    ctx.noted.note(collection, &synced.0);
    Ok(synced)
}

/// Marks a sync whose listing found the source renumbered every member, which
/// a rekey of the collection settles before it is listed again.
#[derive(Debug)]
struct Renumbered;

impl fmt::Display for Renumbered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("The source renumbered the collection")
    }
}

/// Drives the engine's rekey, returning its report.
///
/// Every old handle drops as `Rekeyed`, so the generation bump commits with
/// what voided the cached ids (pimdir SYNC §8). A listing leaving most items
/// out parks it, nothing written, unless the run accepts absences.
fn rebuild_collection(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
) -> Result<PimdirRekeyReport> {
    let rekey = PimdirRekey::new(collection.to_string()).accept_absences(ctx.accept_absences);
    let mut remote =
        PimRemote::new(&mut ctx.pool, blobs.clone(), ctx.namespace.clone()).with_capped(ctx.capped);
    let rebuilt =
        run_verb(store, &mut remote, rekey).with_context(|| format!("Rebuild {collection}"));
    ctx.noted.truncated.extend(remote.take_truncated());
    rebuilt
}

/// The `sync` verb one side runs, under its options.
///
/// Being hub-bound fixes the delete disposition: a refused delete is held, not
/// reverted. Reverting says "this source still holds the member", and an add
/// beats a delete, so a side taking no deletes would resurrect on both sides.
#[cfg(test)]
pub(super) fn sync_verb(
    collection: &str,
    push: bool,
    rights: PimdirPushRights,
    conflict: PimdirConflictPolicy,
    scope: PimdirScope,
    scope_bound: bool,
) -> PimdirSync {
    let opts = PimdirSyncOptions {
        push,
        rights,
        conflict,
        scope,
        ..Default::default()
    };

    sync_verb_with(collection, opts, scope_bound)
}

/// [`sync_verb`] under options stated whole.
pub(super) fn sync_verb_with(
    collection: &str,
    opts: PimdirSyncOptions,
    scope_bound: bool,
) -> PimdirSync {
    PimdirSync::new(collection.to_string(), opts)
        .beside_other_sources(true)
        .scope_bound(scope_bound)
}

/// Runs one side's `sync` against its server and returns its report, and
/// whether `pager` paused it.
///
/// The refusals and rejections it met are kept even when the pass fails: a
/// later failure does not unlearn what the run saw.
pub(super) fn sync_side(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    push: bool,
    pager: Option<&mut dyn Pager>,
) -> Result<(PimdirSyncReport, bool)> {
    let bound = store
        .load(
            &PimdirCollectionId(collection.to_string()),
            &PimdirLoadSelection::All,
        )
        .map(Bound::from_loaded)
        .map_err(|err| anyhow!("Load {collection} error: {err}"))?;
    let verb = sync_verb_with(
        collection,
        ctx.sync_options(push),
        ctx.pool.primary().scope_bound(),
    )
    .heads(ctx.pool.primary().heads());
    let mut remote = PimRemote::new(&mut ctx.pool, blobs.clone(), ctx.namespace.clone())
        .with_bound(collection, bound)
        .with_bodies(!ctx.dry_run)
        .with_capped(ctx.capped);
    let report = run_verb_paged(store, &mut remote, verb, pager);
    let renumbered = remote.rekey_asked();
    let refused = remote.take_refused();
    let rejected = remote.take_rejected();
    ctx.refused.extend(refused);
    ctx.rejected.extend(rejected);
    ctx.noted.truncated.extend(remote.take_truncated());

    let report = match report {
        Err(err) if renumbered => Err(err.context(Renumbered)),
        report => report,
    };
    let (report, paused) =
        match report.with_context(|| format!("Sync {} {collection}", ctx.name))? {
            Paged::Done(report) => (report, false),
            Paged::Paused(verb) => (verb.report().clone(), true),
        };

    let wire = wire_name(&ctx.namespace, collection);
    let unnamed = unnamed_creates(&report, wire, &ctx.rejected, &ctx.refused);
    ctx.rejected.extend(unnamed);

    Ok((report, paused))
}

/// The creates the engine says the server rejected (pimdir SYNC §5) that no
/// recorded rejection or refusal names, each as a rejected `create`.
///
/// None today, the connector naming every rejection; it keeps a create a
/// later path rejects silently from being pushed for ever unreported.
pub(super) fn unnamed_creates(
    report: &PimdirSyncReport,
    collection: &str,
    rejected: &[RejectedPush],
    refused: &[RefusedCreate],
) -> Vec<RejectedPush> {
    report
        .rejected_creates
        .iter()
        .filter(|handle| {
            let named = |c: &str, h: &str| c == collection && h == handle.as_str();
            !rejected.iter().any(|r| named(&r.collection, &r.handle))
                && !refused.iter().any(|r| named(&r.collection, &r.handle))
        })
        .map(|handle| RejectedPush {
            collection: collection.to_string(),
            handle: handle.0.clone(),
            action: "create",
            failure: Failure::new("the server rejected it, giving no reason"),
        })
        .collect()
}

/// One side's placements the engine left marked conflicted in `collection`.
pub(super) fn conflicted_placements(
    store: &PimdirSourceStore,
    collection: &str,
    source: &str,
) -> Result<Vec<PimdirPlacement>> {
    Ok(load_side(store, collection)
        .with_context(|| format!("Load {source} {collection}"))?
        .into_iter()
        .filter(|placement| placement.status == PimdirStatus::Conflict)
        .collect())
}

/// Merges what nobody disagreed about, leaving parked only what needs a person.
///
/// Most divergence is not disagreement, as the base proves; the remote body is
/// fetched first, never merged against nothing. A side the store decides gets
/// its body restated instead, the pushing pass never revisiting the conflict.
pub(super) fn resolve_conflicts(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    store_dir: &Path,
    dry_run: bool,
) -> Result<usize> {
    if dry_run {
        return Ok(0);
    }

    let parked = conflicted_placements(store, collection, &ctx.name)?;
    if parked.is_empty() {
        return Ok(0);
    }

    if ctx.authority == Authority::Store {
        let options = ctx.upgrade_options();
        let mut remote = PimRemote::new(&mut ctx.pool, blobs.clone(), ctx.namespace.clone());
        let overruled = &mut ctx.noted.overruled;
        return keep_store_bodies(
            collection,
            &ctx.name,
            store,
            blobs,
            &mut remote,
            options,
            overruled,
        );
    }

    debug!("merge {} conflicted item(s) in {collection}", parked.len());

    let wanted: Vec<PimdirHandle> = parked
        .iter()
        .filter(|placement| placement.conflict_object.is_none())
        .map(|placement| placement.handle.clone())
        .collect();

    if !wanted.is_empty() {
        let options = ctx.upgrade_options();
        let bound = store
            .load(
                &PimdirCollectionId(collection.to_string()),
                &PimdirLoadSelection::All,
            )
            .map(Bound::from_loaded)
            .map_err(|err| anyhow!("Load {collection} error: {err}"))?;
        let mut remote = PimRemote::new(&mut ctx.pool, blobs.clone(), ctx.namespace.clone())
            .with_bound(collection, bound);
        let upgrade =
            PimdirUpgrade::new(collection.to_string(), wanted, PimdirTier::Full).options(options);
        let upgraded = run_verb(store, &mut remote, upgrade)
            .with_context(|| format!("Fetch the diverging bodies of {} {collection}", ctx.name))?;
        ctx.noted.note_events(collection, &upgraded.events);
    }

    let kind = resolve_kind(&mut ctx.pool);

    let noted = &mut ctx.noted;
    merge_conflicts(collection, kind, &ctx.name, store, blobs, store_dir, noted)
}

/// Settles every divergence of a side the store decides on the store's body.
///
/// The edit overrules (pimdir SYNC §7) as `conflict resolve --prefer-local`
/// does, each discarded body kept as a retained item noted in `overruled`. The
/// store overrules no unread body, so it is fetched first or stays parked.
pub(super) fn keep_store_bodies<R: PimdirRemote<Error = anyhow::Error>>(
    collection: &str,
    source: &str,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    remote: &mut R,
    options: PimdirUpgradeOptions,
    overruled: &mut Vec<(String, PimdirHandle, PimdirLinkId)>,
) -> Result<usize> {
    let unread: Vec<PimdirHandle> = conflicted_placements(store, collection, source)?
        .into_iter()
        .filter(|placement| placement.conflict_object.is_none())
        .map(|placement| placement.handle)
        .collect();
    let fetched = fetch_diverging(collection, source, store, remote, options, unread.clone())?;
    note_overruled(overruled, collection, fetched.events);

    let parked = conflicted_placements(store, collection, source)?;
    let mut kept = unread
        .iter()
        .filter(|handle| !parked.iter().any(|placement| &placement.handle == *handle))
        .count();

    for placement in parked {
        let handle = placement.handle;
        let name = handle.0.clone();

        let Some(hash) = placement.object else {
            debug!("conflicted item {name} in {collection} holds no body to keep");
            continue;
        };
        let Some(body) = blobs
            .blobs()
            .get(&hash)
            .with_context(|| format!("Read the body of {name} in {collection}"))?
        else {
            debug!("the body of the conflicted item {name} in {collection} is not in the store");
            continue;
        };

        let edit = PimdirMutation::Edit {
            handle: handle.clone(),
            object: PimdirObject {
                hash: hash.clone(),
                size: body.len(),
            },
            body,
            summary: None,
            sort_key: None,
            if_match: Some(hash),
            overrule: true,
            resolves: None,
        };

        let mutated = match store.mutate(collection, edit.clone()) {
            Err(PimdirRunError::Store(PimdirError::Unread { .. })) if !unread.contains(&handle) => {
                let fetched =
                    fetch_diverging(collection, source, store, remote, options, vec![handle])?;
                note_overruled(overruled, collection, fetched.events);
                store.mutate(collection, edit)
            }
            mutated => mutated,
        };

        let mutated = match mutated {
            Ok(mutated) => mutated,
            Err(PimdirRunError::Store(PimdirError::Unread { .. })) => {
                debug!("the diverging body of {name} in {collection} is still unread, left parked");
                continue;
            }
            Err(PimdirRunError::Store(err)) => {
                let context = format!("Keep the store's body of {name} in {collection}");
                return Err(pimdir_error(err).context(context));
            }
            Err(err) => bail!("Keep the store's body of {name} in {collection}: {err}"),
        };

        note_overruled(overruled, collection, mutated.events);

        kept += 1;
    }

    if kept > 0 {
        debug!("kept the store's body over {kept} divergence(s) in {collection}");
    }

    Ok(kept)
}

/// Notes the bodies `events` say a policy kept and did not adopt.
fn note_overruled(
    overruled: &mut Vec<(String, PimdirHandle, PimdirLinkId)>,
    collection: &str,
    events: Vec<PimdirSyncEvent>,
) {
    for event in events {
        if let PimdirSyncEvent::Overruled { handle, link_id } = event {
            overruled.push((collection.to_string(), handle, link_id));
        }
    }
}

/// Fetches the diverging bodies of `handles`, held as wanted only.
///
/// A settle over them then keeps what it discards; the fetch settles what it
/// reads under `options` (pimdir SYNC §6).
pub(super) fn fetch_diverging<R: PimdirRemote<Error = anyhow::Error>>(
    collection: &str,
    source: &str,
    store: &mut PimdirSourceStore,
    remote: &mut R,
    options: PimdirUpgradeOptions,
    handles: Vec<PimdirHandle>,
) -> Result<PimdirUpgradeReport> {
    if handles.is_empty() {
        return Ok(PimdirUpgradeReport::default());
    }

    debug!(
        "fetch {} diverging body(ies) in {collection}",
        handles.len()
    );
    let upgrade =
        PimdirUpgrade::new(collection.to_string(), handles, PimdirTier::Full).options(options);
    run_verb(store, remote, upgrade)
        .with_context(|| format!("Fetch the diverging bodies of {source} {collection}"))
}

/// The half of [`resolve_conflicts`] that reaches no server.
///
/// Stages each merge through the queue as an owner's edit, matched on the
/// local body so an edit made since parks, then judges it by its own row:
/// the drain is the store's, its other rows of the account land in `noted`.
pub(super) fn merge_conflicts(
    collection: &str,
    kind: Kind,
    source: &str,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    store_dir: &Path,
    noted: &mut Noted,
) -> Result<usize> {
    let mut producer = PimdirProducer::open(store_dir, env!("CARGO_PKG_NAME"))
        .with_context(|| format!("Stage the merged conflicts of {collection}"))?;
    let mut staged = Vec::new();

    for placement in conflicted_placements(store, collection, source)? {
        let handle = placement.handle.0.clone();

        let Some(link_id) = placement.link_id.clone() else {
            debug!("conflicted item {handle} in {collection} carries no link id yet");
            continue;
        };

        let sides = (
            placement.base.as_ref().and_then(|base| base.object.clone()),
            placement.object.clone(),
            placement.conflict_object.clone(),
        );
        let (Some(base_hash), Some(local_hash), Some(remote_hash)) = sides else {
            debug!("conflicted item {handle} in {collection} is missing a side to merge against");
            continue;
        };

        let read = |hash: &PimdirHash| -> Result<Option<Vec<u8>>> {
            blobs.blobs().get(hash).with_context(|| {
                format!(
                    "Read the body {} of {handle} in {collection}",
                    hash.as_str()
                )
            })
        };
        let (Some(base), Some(local), Some(remote)) =
            (read(&base_hash)?, read(&local_hash)?, read(&remote_hash)?)
        else {
            debug!("a body of the conflicted item {handle} in {collection} is not in the store");
            continue;
        };

        let body = match kind.merge(&base, &local, &remote) {
            Merged::Body(body) => body,
            Merged::Collided(fields) => {
                debug!("both sides changed {fields} field(s) of {handle} in {collection}");
                continue;
            }
            Merged::Unmergeable(why) => {
                debug!("cannot merge {handle} in {collection}: {why}");
                continue;
            }
        };

        let Some(seq) = store
            .seq_for_link(collection, &link_id.0)
            .with_context(|| format!("Resolve the id of {handle} in {collection}"))?
        else {
            debug!("conflicted item {handle} in {collection} has no row to update");
            continue;
        };

        let enqueued = producer
            .enqueue_with_body(collection, &body, |object| PimdirAction::Update {
                seq,
                object,
                if_match: Some(local_hash),
                resolves: Some(vec![remote_hash.clone()]),
            })
            .with_context(|| format!("Stage the merged body of {handle} in {collection}"))?;

        staged.push(enqueued.handle.id);
    }

    drop(producer);

    if staged.is_empty() {
        return Ok(0);
    }

    let mut drained = SyncOutput::default();
    drain_queues_into(store, &mut drained)
        .with_context(|| format!("Apply the merged conflicts of {collection}"))?;

    let mut merged = 0usize;
    for id in staged {
        let status = store
            .action_status(id)
            .map_err(pimdir_error)
            .with_context(|| format!("Read where the merge #{id} in {collection} stands"))?;
        match status {
            PimdirActionStatus::Applied { .. } => merged += 1,
            PimdirActionStatus::Parked { error, .. } => {
                warn!("merged conflict #{id} in {collection} parked: {error}");
            }
            _ => warn!("merged conflict #{id} in {collection} was left queued"),
        }
    }
    info!("merged and resolved {merged} conflict(s) in {collection}");

    let owners = collection_accounts(store);
    let account = owners.get(collection);
    let owned = |name: &str| account.is_some() && owners.get(name) == account;
    for mut entry in drained.drained {
        if entry.collection == collection {
            entry.applied = entry.applied.saturating_sub(merged);
        }
        if entry.applied > 0 && owned(&entry.collection) {
            noted.drained.push(entry);
        }
    }
    let overruled = drained.overruled.into_iter();
    noted
        .drained_overruled
        .extend(overruled.filter(|body| owned(&body.collection)));

    Ok(merged)
}
