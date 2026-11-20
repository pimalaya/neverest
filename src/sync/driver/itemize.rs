//! # Itemized report
//!
//! What the engine did, translated back into the report's hunks: pulls,
//! conflicts, copies, refused creates and rejected writes.

use crate::sync::driver::*;

/// A `handle -> flags` snapshot of a side's items, taken before a pull.
///
/// Its remote flag changes are then diffed out of the sync's per-item events.
pub(super) fn flag_snapshot(
    store: &PimdirSourceStore,
    collection: &str,
    source: &str,
) -> Result<HashMap<String, PimdirFlags>> {
    Ok(load_side(store, collection)
        .with_context(|| format!("Load {source} {collection}"))?
        .into_iter()
        .map(|placement| (placement.handle.0, placement.flags))
        .collect())
}

/// Itemizes the remote-originated flag changes and removals a pull applied.
///
/// The pull applies them silently, the item reading `Clean` afterwards, so they
/// are recovered from the sync's per-item events. A new remote item is an
/// `Added` event but the pull plan already reports it as a `Fetch`.
pub(super) fn itemize_pulled(
    events: &[PimdirSyncEvent],
    before: &HashMap<String, PimdirFlags>,
    store: &PimdirSourceStore,
    collection: &str,
    display: &str,
    source: &str,
    report: &mut SyncOutput,
) -> Result<()> {
    let after = flag_snapshot(store, collection, source)?;
    itemize_conflicted(events, store, collection, display, source, report)?;

    for event in events {
        match event {
            PimdirSyncEvent::FlagsChanged(handle) => {
                let old = before.get(&handle.0).cloned().unwrap_or_default();
                let Some(new) = after.get(&handle.0) else {
                    continue;
                };
                let (added, removed) = flag_diff(&old, new);
                if !added.is_empty() {
                    report.item_patch.push(PatchEntry::new(
                        ItemHunk::AddFlags {
                            side: source.to_string(),
                            collection: display.to_string(),
                            id: handle.0.clone(),
                            flags: added,
                            direction: Direction::Pull,
                            content_key: 0,
                        },
                        None,
                    ));
                }
                if !removed.is_empty() {
                    report.item_patch.push(PatchEntry::new(
                        ItemHunk::RemoveFlags {
                            side: source.to_string(),
                            collection: display.to_string(),
                            id: handle.0.clone(),
                            flags: removed,
                            direction: Direction::Pull,
                            content_key: 0,
                        },
                        None,
                    ));
                }
            }
            PimdirSyncEvent::Vanished(handle) => {
                report.item_patch.push(PatchEntry::new(
                    ItemHunk::Delete {
                        side: source.to_string(),
                        collection: display.to_string(),
                        id: handle.0.clone(),
                        direction: Direction::Pull,
                        content_key: 0,
                    },
                    None,
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Names the divergences a pass parked, once each.
///
/// A `Conflicted` event says a placement entered conflict, not that it is still
/// in one, [`resolve_conflicts`] having run since, so the store is asked which
/// survived. It notes rather than pushes: one divergence stays one line.
pub(super) fn itemize_conflicted(
    events: &[PimdirSyncEvent],
    store: &PimdirSourceStore,
    collection: &str,
    display: &str,
    source: &str,
    report: &mut SyncOutput,
) -> Result<()> {
    if !events
        .iter()
        .any(|event| matches!(event, PimdirSyncEvent::Conflicted(_)))
    {
        return Ok(());
    }

    let parked: HashSet<String> = conflicted_placements(store, collection, source)?
        .into_iter()
        .map(|placement| placement.handle.0)
        .collect();

    for event in events {
        let PimdirSyncEvent::Conflicted(handle) = event else {
            continue;
        };

        if !parked.contains(&handle.0) {
            continue;
        }

        report.note_conflict(ItemConflict {
            side: source.to_string(),
            collection: display.to_string(),
            id: handle.0.clone(),
        });
    }

    Ok(())
}

/// Itemizes one side's projection: the work a flag, move or compose staged.
pub(super) fn itemize_single(
    collection: &str,
    store: &PimdirStore,
    ctx: &SourceCtx,
    report: &mut SyncOutput,
) -> Result<()> {
    let display = display_name(&ctx.namespace, collection);
    let view = projection_view(store, collection, &ctx.name)
        .with_context(|| format!("Project {} {display}", ctx.name))?;
    let mut forbidden = BTreeMap::new();
    for placement in view {
        for hunk in placement_hunks(&ctx.name, &ctx.name, &ctx.namespace, display, &placement) {
            if stays_local(ctx, &hunk) {
                continue;
            }
            match forbidden_action(&ctx.perms, &hunk) {
                Some(action) => *forbidden.entry(action).or_default() += 1,
                None => report.item_patch.push(PatchEntry::new(hunk, None)),
            }
        }
    }
    report_forbidden(ctx, display, forbidden, report);
    Ok(())
}

/// Whether `hunk` is a flag change on a kind whose backend carries no flags,
/// the store's alone: never pushed, so neither reported nor forbidden.
fn stays_local(ctx: &SourceCtx, hunk: &ItemHunk) -> bool {
    let flags = matches!(
        hunk,
        ItemHunk::AddFlags { .. } | ItemHunk::RemoveFlags { .. }
    );
    flags && ctx.pool.media_type() != "message/rfc822"
}

/// The action `hunk` names when the source's rights forbid it, the engine
/// then holding it in the store rather than pushing it.
fn forbidden_action(perms: &SourcePermissions, hunk: &ItemHunk) -> Option<&'static str> {
    match hunk {
        ItemHunk::Add { .. } | ItemHunk::Copy { .. } if !perms.item.create => Some("create"),
        ItemHunk::Update { .. } if !perms.item.update => Some("update"),
        ItemHunk::Delete { .. } if !perms.item.delete => Some("delete"),
        ItemHunk::AddFlags { .. } | ItemHunk::RemoveFlags { .. } if !perms.flag.update => {
            Some("set-flags")
        }
        _ => None,
    }
}

/// Reports the writes [`forbidden_action`] held back on `ctx`, one row an
/// action.
fn report_forbidden(
    ctx: &SourceCtx,
    collection: &str,
    forbidden: BTreeMap<&'static str, usize>,
    report: &mut SyncOutput,
) {
    for (action, count) in forbidden {
        report.forbidden.push(ForbiddenWrites {
            side: ctx.name.clone(),
            collection: collection.to_string(),
            kind: action.to_string(),
            count,
        });
    }
}

/// Reports the pull plan: every bodiless live item, and every item the pull
/// brought in with its body.
///
/// A kind named by its body (DAV, Google, Graph contacts and events) arrives
/// with it, so those are read off the pull's events, and off `owed`: what was
/// bodiless before the pull and hydrated since.
pub(super) fn itemize_fetches(
    collection: &str,
    display: &str,
    store: &PimdirSourceStore,
    source: &str,
    events: &[PimdirSyncEvent],
    owed: &HashSet<String>,
    report: &mut SyncOutput,
) -> Result<()> {
    let pulled: HashSet<&str> = events
        .iter()
        .filter_map(|event| match event {
            PimdirSyncEvent::Added(handle) | PimdirSyncEvent::ContentChanged(handle) => {
                Some(handle.as_str())
            }
            _ => None,
        })
        .collect();
    let view =
        load_side(store, collection).with_context(|| format!("Load {source} {collection}"))?;
    for placement in view {
        let arrived = placement.object.is_some()
            && (pulled.contains(placement.handle.as_str())
                || owed.contains(placement.handle.as_str()));
        if placement.status == PimdirStatus::Tombstone || (placement.object.is_some() && !arrived) {
            continue;
        }
        let id = placement
            .link_id
            .as_ref()
            .map(|l| l.0.clone())
            .unwrap_or_else(|| placement.handle.0.clone());
        report.item_patch.push(PatchEntry::new(
            ItemHunk::Fetch {
                side: source.to_string(),
                collection: display.to_string(),
                id,
                content_key: 0,
            },
            None,
        ));
    }
    Ok(())
}

/// What a side's syncs reported beyond their item events, drained into the
/// run's report by [`itemize_noted`], keyed by hub collection id.
///
/// The passes of one collection meet each again, so each is kept once.
#[derive(Default)]
pub(super) struct Noted {
    /// The listings that parked their absences, and whether each was a
    /// rebuild's.
    parked: BTreeMap<String, (PimdirParked, bool)>,
    /// The most changes one pass withheld over an unreadable body.
    unreadable: BTreeMap<String, usize>,
    /// The bodies a policy did not adopt: the handle whose write settled
    /// each, and the retained item keeping it.
    pub(super) overruled: Vec<(String, PimdirHandle, PimdirLinkId)>,
    /// The pushes the last pass left unanswered, which the next run pushes
    /// again: an earlier pass's are pushed again by the later ones.
    unanswered: BTreeMap<String, usize>,
    /// The creates the last pass left waiting (pimdir SYNC §5): one an
    /// earlier pass left waiting may have landed since.
    waiting: BTreeMap<String, Vec<PimdirWaiting>>,
    /// The members named apart from a create holding their identity (pimdir
    /// SYNC §5, §6): the member's handle and the create's key.
    disagreed: Vec<(String, PimdirHandle, PimdirLinkId)>,
    /// The changes the remote refused as they stand (pimdir SYNC §5), with
    /// the last reason a pass gave.
    refusals: BTreeMap<(String, PimdirHandle), String>,
    /// The members a capped round did not list, and so kept (pimdir SYNC
    /// §5): the most one pass counted.
    unlisted: BTreeMap<String, usize>,
    /// The bodies a batch downloaded and could not land in the store.
    pub(super) failures: Vec<StepFailure>,
    /// The collections whose listing the server cut short, by wire name.
    pub(super) truncated: BTreeSet<String>,
    /// The account's rows a mid-run drain applied beside the run's merges.
    pub(super) drained: Vec<DrainedQueue>,
    /// The bodies that drain kept aside.
    pub(super) drained_overruled: Vec<OverruledBody>,
}

impl Noted {
    /// Notes a listing of `collection` that parked.
    pub(super) fn park(&mut self, collection: &str, parked: PimdirParked, rebuild: bool) {
        self.parked
            .insert(collection.to_string(), (parked, rebuild));
    }

    /// Notes what one sync of `collection` reported.
    pub(super) fn note(&mut self, collection: &str, report: &PimdirSyncReport) {
        if let Some(parked) = report.parked {
            warn!(
                "{} of {} items of {collection} absent from its listing, parked",
                parked.absent, parked.bound
            );
            self.park(collection, parked, false);
        }
        if report.unreadable > 0 {
            let withheld = self.unreadable.entry(collection.to_string()).or_default();
            *withheld = (*withheld).max(report.unreadable);
        }
        self.unanswered
            .insert(collection.to_string(), report.unanswered);
        if report.unlisted > 0 {
            debug!(
                "{} items of {collection} unlisted by its capped source, kept",
                report.unlisted
            );
            let kept = self.unlisted.entry(collection.to_string()).or_default();
            *kept = (*kept).max(report.unlisted);
        }
        for waiting in &report.waiting {
            debug!(
                "create {} of {collection} waits: {:?}",
                waiting.handle.as_str().trim_start_matches('\u{1}'),
                waiting.reason
            );
        }
        self.waiting
            .insert(collection.to_string(), report.waiting.clone());
        self.note_events(collection, &report.events);
    }

    /// Notes, each once, the bodies `events` say a policy did not adopt and
    /// the members named apart from a create holding their identity.
    pub(super) fn note_events(&mut self, collection: &str, events: &[PimdirSyncEvent]) {
        for event in events {
            match event {
                PimdirSyncEvent::Overruled { handle, link_id } => {
                    let kept = (collection.to_string(), handle.clone(), link_id.clone());
                    if !self.overruled.contains(&kept) {
                        self.overruled.push(kept);
                    }
                }
                PimdirSyncEvent::Disagreed { handle, link_id } => {
                    warn!(
                        "{} of {collection} disagrees with the create {}, kept apart from it",
                        handle.as_str(),
                        link_id.as_str()
                    );
                    let apart = (collection.to_string(), handle.clone(), link_id.clone());
                    if !self.disagreed.contains(&apart) {
                        self.disagreed.push(apart);
                    }
                }
                PimdirSyncEvent::Refused { handle, reason } => {
                    let key = (collection.to_string(), handle.clone());
                    self.refusals.insert(key, reason.clone());
                }
                PimdirSyncEvent::Abandoned(handle) => {
                    let key = handle.as_str().trim_start_matches('\u{1}');
                    info!("the delivered create {key} of {collection} was given up");
                }
                _ => {}
            }
        }
    }
}

/// Drains what `ctx`'s syncs noted into `report`.
///
/// A create is held under its provisional handle, U+0001 then its key (pimdir
/// SYNC §2), and is named by that key, the part a person can read.
pub(super) fn itemize_noted(ctx: &mut SourceCtx, report: &mut SyncOutput) {
    let Noted {
        parked,
        unreadable,
        overruled,
        unanswered,
        waiting,
        disagreed,
        unlisted,
        refusals,
        failures,
        truncated,
        drained,
        drained_overruled,
    } = mem::take(&mut ctx.noted);
    let display = |collection: &str| display_name(&ctx.namespace, collection).to_string();
    report.failures.extend(failures);
    report.drained.extend(drained);
    report.overruled.extend(drained_overruled);
    for collection in truncated {
        let err = anyhow!("The server cut the listing short, the members past the cut unseen");
        let failure = StepFailure::new(FailedStep::Listing, Some(collection), &err);
        report.failures.push(failure);
    }

    for ((collection, handle), reason) in refusals {
        let id = handle.as_str().trim_start_matches('\u{1}').to_owned();
        let collection = display(&collection);
        report
            .item_patch
            .retain(|entry| !names(&entry.hunk, &ctx.name, &collection, &id));
        report
            .refusals
            .retain(|r| r.side != ctx.name || r.collection != collection || r.id != id);
        report.refusals.push(RefusedChange {
            side: ctx.name.clone(),
            collection,
            id,
            reason,
        });
    }

    for (collection, (parked, rebuild)) in parked {
        report.absences.push(ParkedAbsence {
            side: ctx.name.clone(),
            collection: display(&collection),
            absent: parked.absent,
            bound: parked.bound,
            rebuild,
        });
    }
    for (collection, count) in unreadable {
        report.unreadable.push(UnreadableBodies {
            side: ctx.name.clone(),
            collection: display(&collection),
            count,
        });
    }
    for (collection, handle, kept) in overruled {
        report.overruled.push(OverruledBody {
            side: Some(ctx.name.clone()),
            collection: display(&collection),
            id: Some(handle.0),
            action: None,
            kept: kept.0,
        });
    }
    for (collection, count) in unanswered {
        if count == 0 {
            continue;
        }
        report.unanswered.push(UnansweredPushes {
            side: ctx.name.clone(),
            collection: display(&collection),
            count,
        });
    }
    for (collection, creates) in waiting {
        let named = display(&collection);
        report
            .waiting
            .retain(|create| create.side != ctx.name || create.collection != named);
        for create in creates {
            let key = create.handle.as_str().trim_start_matches('\u{1}');
            report.waiting.push(WaitingCreate {
                side: ctx.name.clone(),
                collection: named.clone(),
                id: key.to_owned(),
                reason: match create.reason {
                    PimdirWaitReason::OutOfScope => WaitReason::OutOfScope,
                    PimdirWaitReason::Delivered => WaitReason::Delivered,
                    PimdirWaitReason::Undeliverable => WaitReason::Undeliverable,
                    _ => WaitReason::Other,
                },
            });
        }
    }
    for (collection, handle, create) in disagreed {
        report.disagreed.push(DisagreedMember {
            side: ctx.name.clone(),
            collection: display(&collection),
            id: handle.0,
            create: create.0,
        });
    }
    for (collection, count) in unlisted {
        report.unlisted.push(UnlistedMembers {
            side: ctx.name.clone(),
            collection: display(&collection),
            count,
        });
    }
}

/// Itemizes the pending cross-side work from each side's hub projection.
///
/// A `Created` placement is a copy in, a `Dirty` one a flag change or an
/// update, a `Tombstone` a delete.
pub(super) fn itemize(
    collection: &str,
    store: &PimdirStore,
    left: &SourceCtx,
    right: &SourceCtx,
    report: &mut SyncOutput,
) -> Result<()> {
    for (ctx, other) in [(left, right), (right, left)] {
        let display = display_name(&ctx.namespace, collection);
        let view = projection_view(store, collection, &ctx.name)
            .with_context(|| format!("Project {} {display}", ctx.name))?;
        let mut forbidden = BTreeMap::new();
        for placement in view {
            for hunk in placement_hunks(&ctx.name, &other.name, &ctx.namespace, display, &placement)
            {
                if stays_local(ctx, &hunk) {
                    continue;
                }
                match forbidden_action(&ctx.perms, &hunk) {
                    Some(action) => *forbidden.entry(action).or_default() += 1,
                    None => report.item_patch.push(PatchEntry::new(hunk, None)),
                }
            }
        }
        report_forbidden(ctx, display, forbidden, report);
    }
    Ok(())
}

/// Maps a projected placement to its report hunks.
///
/// A flag change can surface as both an add and a remove. A create is a
/// server-side copy from its origin, an upload of the other side's body, or,
/// with no other side, an append a frontend authored (pimdir SYNC §5).
pub(super) fn placement_hunks(
    source: &str,
    other: &str,
    namespace: &str,
    collection: &str,
    placement: &PimdirPlacement,
) -> Vec<ItemHunk> {
    match placement.status {
        PimdirStatus::Created => {
            let link = placement
                .link_id
                .as_ref()
                .map(|l| l.0.clone())
                .unwrap_or_default();
            let flags = to_email_flag_set(&placement.flags);
            let content_key = content_key(&link);
            let hunk = match &placement.origin {
                Some(origin) => ItemHunk::Copy {
                    source_side: source.to_string(),
                    target_side: source.to_string(),
                    collection: collection.to_string(),
                    source_id: link,
                    flags,
                    origin: Some(wire_name(namespace, &origin.collection.0).to_string()),
                    content_key,
                },
                None if source == other => ItemHunk::Add {
                    side: source.to_string(),
                    collection: collection.to_string(),
                    id: link,
                    flags,
                    content_key,
                },
                None => ItemHunk::Copy {
                    source_side: other.to_string(),
                    target_side: source.to_string(),
                    collection: collection.to_string(),
                    source_id: link,
                    flags,
                    origin: None,
                    content_key,
                },
            };
            vec![hunk]
        }
        PimdirStatus::Tombstone => vec![ItemHunk::Delete {
            side: source.to_string(),
            collection: collection.to_string(),
            id: placement.handle.0.clone(),
            direction: Direction::Push,
            content_key: 0,
        }],
        PimdirStatus::Dirty => {
            let base = placement
                .base
                .as_ref()
                .map(|b| b.flags.clone())
                .unwrap_or_default();
            let (added, removed) = flag_diff(&base, &placement.flags);
            let mut hunks = Vec::new();
            if !added.is_empty() {
                hunks.push(ItemHunk::AddFlags {
                    side: source.to_string(),
                    collection: collection.to_string(),
                    id: placement.handle.0.clone(),
                    flags: added,
                    direction: Direction::Push,
                    content_key: 0,
                });
            }
            if !removed.is_empty() {
                hunks.push(ItemHunk::RemoveFlags {
                    side: source.to_string(),
                    collection: collection.to_string(),
                    id: placement.handle.0.clone(),
                    flags: removed,
                    direction: Direction::Push,
                    content_key: 0,
                });
            }
            let base_object = placement.base.as_ref().and_then(|b| b.object.as_ref());
            if placement.object.as_ref() != base_object {
                hunks.push(ItemHunk::Update {
                    side: source.to_string(),
                    collection: collection.to_string(),
                    id: placement.handle.0.clone(),
                    content_key: 0,
                });
            }
            hunks
        }
        PimdirStatus::Clean | PimdirStatus::Conflict => Vec::new(),
    }
}

/// Names the creates a side refused because it already holds the identity.
///
/// A collection reconciles until quiescent, so a create the other side will not
/// take is re-refused every pass. Two copies of one identity are still two
/// refusals, which is what the handle tells apart from a repeated one.
pub(super) fn itemize_refused(side: &str, refused: Vec<RefusedCreate>, report: &mut SyncOutput) {
    let mut seen: HashSet<(String, String, String)> = HashSet::new();

    for refused in refused {
        let key = (
            refused.collection.clone(),
            refused.uid.clone(),
            refused.handle,
        );

        if !seen.insert(key) {
            continue;
        }

        report.duplicates.push(RefusedDuplicate {
            side: side.to_string(),
            collection: refused.collection,
            uid: refused.uid,
        });
    }
}

/// Names the writes a side would not take, un-counting their hunks.
///
/// The item patch is a plan itemized before any push, so a refused write left
/// in it comes back as a phantom on every later run. A create is named by its
/// provisional handle's key, past the U+0001 (pimdir SYNC §2).
pub(super) fn itemize_rejected(side: &str, rejected: Vec<RejectedPush>, report: &mut SyncOutput) {
    let mut seen: HashSet<(String, String, &'static str)> = HashSet::new();

    for rejected in rejected {
        let id = match rejected.handle.strip_prefix('\u{1}') {
            Some(link) => link.to_string(),
            None => rejected.handle.clone(),
        };
        let key = (rejected.collection.clone(), id.clone(), rejected.action);

        if !seen.insert(key) {
            continue;
        }

        report
            .item_patch
            .retain(|entry| !names(&entry.hunk, side, &rejected.collection, &id));
        let Failure {
            code,
            error,
            sources,
        } = rejected.failure;
        report.rejected.push(RejectedWrite {
            side: side.to_string(),
            collection: rejected.collection,
            kind: rejected.action.to_string(),
            failure: ItemError {
                id: Some(id),
                code,
                error,
                sources,
            },
        });
    }
}

/// Whether `hunk` is the plan for `handle` in `collection` on `side`.
///
/// A copy carries no handle on the side it is being created on, so it is named
/// by the item's link id, which is what an append the hub staged is keyed by
/// too. A fetch and a pulled change reach no server and are never rejected.
fn names(hunk: &ItemHunk, side: &str, collection: &str, handle: &str) -> bool {
    match hunk {
        ItemHunk::AddFlags {
            side: s,
            collection: c,
            id,
            direction: Direction::Push,
            ..
        }
        | ItemHunk::RemoveFlags {
            side: s,
            collection: c,
            id,
            direction: Direction::Push,
            ..
        }
        | ItemHunk::Delete {
            side: s,
            collection: c,
            id,
            direction: Direction::Push,
            ..
        }
        | ItemHunk::Update {
            side: s,
            collection: c,
            id,
            ..
        } => s == side && c == collection && id == handle,
        ItemHunk::Add {
            side: s,
            collection: c,
            id,
            ..
        } => s == side && c == collection && id == handle,
        ItemHunk::Copy {
            target_side,
            collection: c,
            source_id,
            ..
        } => target_side == side && c == collection && source_id == handle,
        ItemHunk::AddFlags { .. }
        | ItemHunk::RemoveFlags { .. }
        | ItemHunk::Delete { .. }
        | ItemHunk::Fetch { .. } => false,
    }
}

pub(super) fn to_email_flag_set(flags: &PimdirFlags) -> BTreeSet<Flag> {
    let Some(flags) = flags.known() else {
        return BTreeSet::new();
    };

    flags.iter().map(|s| Flag::from_raw(s.clone())).collect()
}

/// What `new` gained over `old`, and what it lost.
///
/// An unknown set holds no markers to compare with, so it reads as empty:
/// nothing is reported added or removed against a side nobody read.
fn flag_diff(old: &PimdirFlags, new: &PimdirFlags) -> (BTreeSet<Flag>, BTreeSet<Flag>) {
    let empty = BTreeSet::new();
    let old = old.known().unwrap_or(&empty);
    let new = new.known().unwrap_or(&empty);
    let diff = |from: &BTreeSet<String>, to: &BTreeSet<String>| {
        from.difference(to)
            .map(|flag| Flag::from_raw(flag.clone()))
            .collect()
    };

    (diff(new, old), diff(old, new))
}

/// A u64 content key for the report DTO, never displayed, so only consistent.
pub(super) fn content_key(link: &str) -> u64 {
    let mut acc: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in link.bytes() {
        acc ^= byte as u64;
        acc = acc.wrapping_mul(0x0000_0100_0000_01b3);
    }
    acc
}
