//! # Side sync
//!
//! One side's run of the engine, rekeying a renumbered collection, and the
//! conflicts it leaves, resolved or merged.

use crate::sync::driver::*;

/// Runs one side's sync, rekeying the collection first when its listing
/// found the source renumbered every member (pimdir SYNC §4, §8).
///
/// The listing fails before anything listed under the new handles lands; the
/// rekey carries every placement onto them by link id, then the sync runs
/// again. A rekey that fails leaves the stored checkpoint as it was, so the
/// next run meets the renumbering again and rekeys then. Also answers whether
/// `pager` paused the sync, its round left open.
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
            // NOTE: a parked rekey wrote nothing, so the collection stays
            // under its old handles and is not listed again this run.
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
/// The rebuild batch drops every old handle as `Rekeyed`, which makes the
/// store bump the collection's generation in the same transaction (pimdir
/// SYNC §8), so "the ids you cached are void" commits with what voided them.
/// A listing leaving most of the collection's items out parks the rekey,
/// nothing written, unless the run accepts absences.
fn rebuild_collection(
    collection: &str,
    ctx: &mut SourceCtx,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
) -> Result<PimdirRekeyReport> {
    let rekey = PimdirRekey::new(collection.to_string()).accept_absences(ctx.accept_absences);
    let mut remote =
        PimRemote::new(&mut ctx.pool, blobs.clone(), ctx.namespace.clone()).with_capped(ctx.capped);
    run_verb(store, &mut remote, rekey).with_context(|| format!("Rebuild {collection}"))
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
    // NOTE: kept whether the pass succeeded or not: a later failure does not
    // unlearn a refusal the run already saw.
    let refused = remote.take_refused();
    let rejected = remote.take_rejected();
    ctx.refused.extend(refused);
    ctx.rejected.extend(rejected);

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

/// The creates the engine says `collection`'s server rejected (pimdir SYNC
/// §5) that no rejection or refusal the connector recorded names, each as a
/// rejected `create`.
///
/// The connector records every rejection it answers with its reason, so this
/// names none today; it keeps a create a later path rejects silently from
/// being pushed for ever unreported.
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
/// Most divergence is not disagreement: one side changed a phone number and the
/// other a note, and the base proves it. The remote body is only wanted, not
/// held, so it is fetched first and never merged against nothing.
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

    // NOTE: the engine records a store-wins divergence as a conflict on a pass
    // that cannot push, and the pushing pass never revisits it, so the store's
    // body is restated here for that pass to push over the endpoint.
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
        let mut remote = PimRemote::new(&mut ctx.pool, blobs.clone(), ctx.namespace.clone());
        let upgrade =
            PimdirUpgrade::new(collection.to_string(), wanted, PimdirTier::Full).options(options);
        let upgraded = run_verb(store, &mut remote, upgrade)
            .with_context(|| format!("Fetch the diverging bodies of {} {collection}", ctx.name))?;
        ctx.noted.note_events(collection, &upgraded.events);
    }

    let kind = resolve_kind(&mut ctx.pool);

    merge_conflicts(collection, kind, &ctx.name, store, blobs, store_dir)
}

/// Settles every divergence of a side the store decides on the store's body.
///
/// Restating the held body is the decision `conflict resolve --prefer-local`
/// takes: the conflict clears against the revision the endpoint holds, so the
/// next pushing pass overwrites it. Nothing is merged, the endpoint's own edit
/// being what `one-way` discards. The edit is the engine's own rather than an
/// owner's through the queue, which a one-way account refuses.
///
/// The edit overrules (pimdir SYNC §7): the store keeps each body it discards,
/// the endpoint's own edit among them, as a retained item, noted into
/// `overruled` with the handle whose write settled it and the retained item's
/// key. A diverging body nobody has read yet is fetched from `remote` first,
/// as [`resolve_conflicts`] does for a merge, since the store refuses to
/// overrule a body it cannot keep; one still unread after its fetch stays
/// parked for a person. The fetch itself settles what it reads under
/// `options` (pimdir SYNC §6), the side's `prefer-local` keeping the
/// endpoint's body as the edit would, and such a divergence counts as kept.
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

/// Fetches the diverging bodies of `handles`, which the store holds as
/// wanted only, so a settle over them keeps what it discards; the fetch
/// settles what it reads under `options` (pimdir SYNC §6).
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
/// Merges the placements whose three bodies the store holds and stages each
/// result through the queue, the same path an owner's own edit takes, so a
/// merged body is written exactly one way. What it cannot settle stays parked.
pub(super) fn merge_conflicts(
    collection: &str,
    kind: Kind,
    source: &str,
    store: &mut PimdirSourceStore,
    blobs: &PimdirBlobStage,
    store_dir: &Path,
) -> Result<usize> {
    // NOTE: opened before the first blob write, its staging lock being what
    // keeps a collector out of the window between a body reaching the blob
    // tree and the queue row pinning it.
    let mut producer = PimdirProducer::open(store_dir, env!("CARGO_PKG_NAME"))
        .with_context(|| format!("Stage the merged conflicts of {collection}"))?;
    let mut staged = 0usize;

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

        // NOTE: merged over the local side, so an edit that replaced it since
        // parks rather than being dropped unmerged; it resolves the remote
        // side it merged, and a conflict it never read parks it.
        producer
            .enqueue_with_body(collection, &body, |object| PimdirAction::Update {
                seq,
                object,
                if_match: Some(local_hash),
                resolves: Some(vec![remote_hash.clone()]),
            })
            .with_context(|| format!("Stage the merged body of {handle} in {collection}"))?;

        staged += 1;
    }

    drop(producer);

    if staged == 0 {
        return Ok(0);
    }

    let drained = store
        .drain()
        .with_context(|| format!("Apply the merged conflicts of {collection}"))?;
    if drained.parked > 0 {
        warn!(
            "{} merged conflict(s) in {collection} could not be applied and parked",
            drained.parked
        );
    }
    info!("merged and resolved {staged} conflict(s) in {collection}");

    Ok(staged)
}
