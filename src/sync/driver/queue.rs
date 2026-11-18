//! # Queue drain
//!
//! What frontends queued, performed on the servers: the store's drain, the
//! collection intents and the calendar invitations.

use crate::sync::driver::*;

/// Drains the pending frontend actions, the whole store's in append order.
///
/// The store applies each action as the source syncing its collection, not as
/// the handle draining (STORAGE §15.2), so whichever side runs first answers
/// for both. The report names the collections whose rows landed.
#[cfg_attr(
    not(any(feature = "smtp", feature = "msgraph", feature = "gmail")),
    allow(dead_code)
)]
pub(crate) fn drain_queues(store: &mut PimdirSourceStore, report: &mut SyncOutput) {
    if let Err(err) = drain_queues_into(store, report) {
        warn!("drain failed, actions stay queued: {err:#}");
        report
            .failures
            .push(StepFailure::new(FailedStep::Queue, None, &err));
    }
}

/// The body of [`drain_queues`], failing on a store step that fails.
pub(super) fn drain_queues_into(
    store: &mut PimdirSourceStore,
    report: &mut SyncOutput,
) -> Result<()> {
    let before = store
        .list_pending_actions()
        .map_err(pimdir_error)
        .context("List the queued actions")?;
    if before.is_empty() {
        return Ok(());
    }

    let drained = store
        .drain()
        .map_err(pimdir_error)
        .context("Drain the queued actions")?;
    if drained.applied > 0 || drained.parked > 0 || drained.skipped > 0 {
        info!(
            "drained {} queued action(s) ({} parked, {} skipped)",
            drained.applied, drained.parked, drained.skipped
        );
    }
    for overruled in drained.overruled {
        warn!(
            "queued action #{} in {} kept a body aside as {}",
            overruled.id, overruled.collection, overruled.link_id.0
        );
        report.overruled.push(OverruledBody {
            side: None,
            collection: overruled.collection,
            id: None,
            action: Some(overruled.id),
            kept: overruled.link_id.0,
        });
    }
    if drained.applied == 0 {
        return Ok(());
    }

    let parked: BTreeSet<i64> = store
        .parked_actions()
        .map_err(pimdir_error)
        .context("List the parked actions")?
        .into_iter()
        .map(|row| row.id)
        .collect();
    let pending: BTreeSet<i64> = store
        .list_pending_actions()
        .map_err(pimdir_error)
        .context("List the queued actions")?
        .into_iter()
        .map(|row| row.id)
        .collect();
    let mut applied: BTreeMap<String, usize> = BTreeMap::new();
    for row in before {
        if !pending.contains(&row.id) && !parked.contains(&row.id) {
            *applied.entry(row.collection).or_default() += 1;
        }
    }
    report.drained.extend(
        applied
            .into_iter()
            .map(|(collection, applied)| DrainedQueue {
                collection,
                applied,
            }),
    );

    Ok(())
}

/// Performs the queued collection intents (pimdir STORAGE Annex B.2), each
/// by the side it names, and settles their rows.
///
/// It runs before the listing, so the same run lists the result. Under
/// `paired` a rename parks and the store follows nothing (pimdir SYNC §5). An
/// intent naming no source goes to its namespace's side, which parks it.
pub(super) fn drain_collection_intents(
    sides: &mut [&mut SourceCtx],
    store: &mut PimdirSourceStore,
    paired: bool,
    report: &mut SyncOutput,
) {
    let intents = match collection::pending(store) {
        Ok(intents) => intents,
        Err(err) => {
            warn!("cannot read the queued collection intents: {err:#}");
            return;
        }
    };

    for intent in &intents {
        let index = match intent.source() {
            Some(name) => sides.iter().position(|ctx| ctx.name == name),
            None => sides.iter().position(|ctx| {
                intent
                    .collection
                    .starts_with(&format!("{}/", ctx.namespace))
            }),
        };
        let Some(index) = index else {
            continue;
        };

        let side = &mut *sides[index];
        let outcome = intent
            .request()
            .and_then(|request| perform_collection_intent(side, intent, &request, paired));
        let row = QueuedIntent {
            id: intent.id,
            collection: &intent.collection,
            kind: intent.kind,
            seq: None,
        };
        let performer = side.name.clone();
        let namespace = side.namespace.clone();
        let (outcome, follow) = match outcome {
            Ok(performed) => (Ok(()), Some(performed)),
            Err(failure) => (Err(failure), None),
        };
        settle_intent(store, row, &performer, outcome, report);

        if paired {
            continue;
        }
        match follow {
            Some(Performed::Renamed(renamed)) => {
                follow_rename(store, &namespace, &intent.collection, &renamed)
            }
            Some(Performed::Deleted) => match store.delete_collection(&intent.collection) {
                Ok(_) => info!("collection {} dropped from the store", intent.collection),
                Err(err) => warn!(
                    "cannot drop the deleted collection {} from the store: {err}",
                    intent.collection
                ),
            },
            Some(Performed::Created) | None => {}
        }
    }
}

/// What a performed collection intent left on the server.
#[derive(Clone, Debug)]
enum Performed {
    Created,
    Renamed(Renamed),
    Deleted,
}

/// Performs one collection intent on `side`'s server, refused when the
/// source's rights now forbid it, as the drain refuses a row.
fn perform_collection_intent(
    side: &mut SourceCtx,
    intent: &CollectionIntent,
    request: &collection::Request,
    paired: bool,
) -> Result<Performed, InvitationFailure> {
    let rights = &side.perms.collection;
    let one_way = side.authority == Authority::Endpoint;
    let refused = match request {
        collection::Request::Create(_) if !rights.create => Some("collection.create"),
        collection::Request::Rename(_) if !rights.renames(one_way) => Some("collection.rename"),
        collection::Request::Delete(_) if !rights.delete => Some("collection.delete"),
        _ => None,
    };
    if let Some(right) = refused {
        return Err(InvitationFailure::Permanent(anyhow!(
            "{right} is disabled on source {}",
            side.name
        )));
    }

    let prefix = format!("{}/", side.namespace);
    let anchor = || match intent.collection.strip_prefix(&prefix) {
        Some(id) => Ok(id.to_owned()),
        None => Err(InvitationFailure::Permanent(anyhow!(
            "Collection {} is not one of source {}",
            intent.collection,
            side.name
        ))),
    };

    match request {
        collection::Request::Create(request) => {
            perform_collection_create(side, request)?;
            Ok(Performed::Created)
        }
        collection::Request::Rename(_) if paired => Err(InvitationFailure::Permanent(anyhow!(
            "A paired account mirrors its collections by name, a rename reading there as a delete and a create"
        ))),
        collection::Request::Rename(request) => {
            let id = anchor()?;
            let renamed = side
                .pool
                .primary()
                .rename_named_collection(&id, &request.name)
                .map_err(InvitationFailure::classify)?;
            info!("collection {id} renamed {} on {}", renamed.id, side.name);
            Ok(Performed::Renamed(renamed))
        }
        collection::Request::Delete(_) => {
            let id = anchor()?;
            side.pool
                .primary()
                .delete_named_collection(&id)
                .map_err(InvitationFailure::classify)?;
            info!("collection {id} deleted on {}", side.name);
            Ok(Performed::Deleted)
        }
    }
}

/// Follows a server rename in the store (pimdir STORAGE Annex B.2).
///
/// Where the id derives from the name (IMAP, Gmail), the collection and those
/// under it take their new id, so their items keep their bindings rather than
/// leaving and arriving again. On failure the next listing reads a new one.
pub(super) fn follow_rename(
    store: &mut PimdirSourceStore,
    namespace: &str,
    anchor: &str,
    renamed: &Renamed,
) {
    let to = hub_id(namespace, &renamed.id);
    if anchor == to {
        if let Err(err) = store.set_collection_name(anchor, &renamed.name) {
            warn!("cannot name {anchor} `{}`: {err}", renamed.name);
        }
        return;
    }

    let collections = match store.list_collections() {
        Ok(collections) => collections,
        Err(err) => return warn!("cannot follow the rename of {anchor} in the store: {err}"),
    };
    let under = renamed
        .delimiter
        .map(|delimiter| format!("{anchor}{delimiter}"));
    for collection in collections {
        let rest = match &under {
            _ if collection.id == anchor => "",
            Some(under) if collection.id.starts_with(under.as_str()) => {
                &collection.id[anchor.len()..]
            }
            _ => continue,
        };
        let id = format!("{to}{rest}");
        let name = match rest {
            "" => renamed.name.clone(),
            rest => format!("{}{rest}", renamed.id),
        };
        if let Err(err) = store.rename_collection(&collection.id, &id) {
            warn!(
                "cannot rename {} to {id} in the store: {err}",
                collection.id
            );
            continue;
        }
        if let Err(err) = store.set_collection_name(&id, &name) {
            warn!("cannot name {id} `{name}`: {err}");
        }
        info!("collection {} renamed {id} in the store", collection.id);
    }
}

/// Creates the collection a collection-create asks for on `side`'s server.
fn perform_collection_create(
    side: &mut SourceCtx,
    request: &PimdirCollectionCreate,
) -> Result<(), InvitationFailure> {
    let prefix = format!("{}/", side.namespace);
    let parent = match &request.parent {
        Some(parent) => match parent.as_str().strip_prefix(&prefix) {
            Some(parent) => Some(parent.to_owned()),
            None => {
                return Err(InvitationFailure::Permanent(anyhow!(
                    "Parent {} is not a collection of source {}",
                    parent.as_str(),
                    side.name
                )));
            }
        },
        None => None,
    };

    let id = side
        .pool
        .primary()
        .create_named_collection(parent.as_deref(), &request.name)
        .map_err(InvitationFailure::classify)?;
    info!("collection {id} created on {}", side.name);
    Ok(())
}

/// Performs the queue's calendar intents, which the store's drain leaves
/// alone (pimdir STORAGE Annex B.2).
///
/// Each runs through the side its payload names, else the namespace's side
/// binding the item, and is marked in flight first: one a killed run left
/// marked parks unconfirmed rather than tell its attendees twice.
pub(super) fn drain_invitations(
    sides: &mut [&mut SourceCtx],
    store: &mut PimdirSourceStore,
    dir: &Path,
    report: &mut SyncOutput,
) {
    let intents = match invitation::pending(store) {
        Ok(intents) => intents,
        Err(err) => {
            warn!("cannot read the queued calendar intents: {err:#}");
            return;
        }
    };
    let in_flight = submit::InFlight::inviting(dir);
    let left = in_flight.left();
    if let Some(intent) = intents.iter().find(|intent| Some(intent.id) == left)
        && !park_interrupted_invitation(store, intent, report)
    {
        return;
    }
    if let Err(err) = in_flight.end() {
        warn!("cannot clear the calendar in-flight mark, no intent runs this run: {err}");
        return;
    }

    for intent in &intents {
        if left == Some(intent.id) {
            continue;
        }
        let located = locate_invitation(store, intent);
        let index = match intent.source() {
            Some(name) => sides.iter().position(|ctx| ctx.name == name),
            None => {
                let candidates: Vec<usize> = (0..sides.len())
                    .filter(|&index| {
                        let prefix = format!("{}/", sides[index].namespace);
                        intent.collection.starts_with(&prefix)
                    })
                    .collect();
                let holder = candidates.iter().copied().find(|&index| {
                    located
                        .as_ref()
                        .is_ok_and(|bindings| bindings.contains_key(&source_id(&sides[index].name)))
                });
                holder.or(candidates.first().copied())
            }
        };
        let Some(index) = index else {
            continue;
        };

        if let Err(err) = in_flight.begin(intent.id) {
            warn!(
                "cannot mark {} intent #{} in flight, it waits: {err}",
                intent.kind, intent.id
            );
            return;
        }
        let side = &mut *sides[index];
        let outcome = located.and_then(|bindings| perform_invitation(side, intent, &bindings));
        let performer = side.name.clone();
        if !settle_invitation(store, intent, &performer, outcome, report) {
            warn!(
                "{} intent #{} keeps its in-flight mark, the next run parks it unconfirmed",
                intent.kind, intent.id
            );
            return;
        }
        if let Err(err) = in_flight.end() {
            warn!("cannot clear the calendar in-flight mark, no other intent runs: {err}");
            return;
        }
    }
}

/// Parks unconfirmed the calendar intent a killed run left in flight, which
/// may have told its attendees already; false when it could not be parked.
fn park_interrupted_invitation(
    store: &mut PimdirSourceStore,
    intent: &InvitationIntent,
    report: &mut SyncOutput,
) -> bool {
    let detail = "the attendees may have been told, so it is never performed again on \
                  its own: a run stopped while performing it";
    warn!("{} intent #{} parked: {detail}", intent.kind, intent.id);
    match store.unconfirm_action(intent.id, detail) {
        Ok(true) => {}
        Ok(false) => warn!(
            "{} intent #{} vanished from the queue before it was parked",
            intent.kind, intent.id
        ),
        Err(err) => {
            warn!("cannot park {} intent #{}: {err}", intent.kind, intent.id);
            return false;
        }
    }
    report.intents.push(IntentEntry {
        id: intent.id,
        collection: intent.collection.clone(),
        kind: intent.kind.clone(),
        source: intent.source().unwrap_or_default().to_string(),
        seq: intent.seq(),
        code: None,
        error: Some(submit::unconfirmed_reason(detail)),
        sources: Vec::new(),
        parked: true,
    });
    true
}

/// Every source's binding of the item an intent addresses.
///
/// An intent keyed by link waits until the drain stages the producer's add.
pub(super) fn locate_invitation(
    store: &PimdirSourceStore,
    intent: &InvitationIntent,
) -> Result<BTreeMap<PimdirSourceId, PimdirBinding>, InvitationFailure> {
    let collection = &intent.collection;
    let transient = |err: PimdirError| {
        InvitationFailure::Transient(anyhow!("Cannot read the item from the store: {err}"))
    };

    let link = match intent.target()? {
        Target::Seq(seq) => match store.get_item(collection, seq).map_err(transient)? {
            Some(item) => item.link_id.0,
            None => {
                return Err(InvitationFailure::Permanent(anyhow!(
                    "Item #{seq} is gone from {collection}"
                )));
            }
        },
        Target::Link(link) => match store.seq_for_link(collection, &link).map_err(transient)? {
            Some(_) => link,
            None => {
                return Err(InvitationFailure::Transient(anyhow!(
                    "No item of {collection} is keyed {link} yet"
                )));
            }
        },
    };

    store.item_bindings(collection, &link).map_err(transient)
}

/// Performs one intent on the item `side` binds.
///
/// An item under its provisional handle waits for its push (pimdir SYNC §4);
/// an occurrence is acted on as its own instance. The last arm reads every
/// value so a build with no calendar backend leaves none unused.
fn perform_invitation(
    side: &mut SourceCtx,
    intent: &InvitationIntent,
    bindings: &BTreeMap<PimdirSourceId, PimdirBinding>,
) -> Result<(), InvitationFailure> {
    let invitation = intent.invitation()?;
    let Some(binding) = bindings.get(&source_id(&side.name)) else {
        return Err(InvitationFailure::Permanent(anyhow!(
            "Source {} does not hold the item {} addresses",
            side.name,
            intent.kind
        )));
    };
    let handle = binding.handle.as_str();
    if binding.base.is_none() || handle.starts_with('\u{1}') {
        return Err(InvitationFailure::Transient(anyhow!(
            "The item is not on {} yet, the intent waits for its push",
            side.name
        )));
    }

    let collection = wire_name(&side.namespace, &intent.collection).to_string();
    let handle = handle.to_string();
    let name = side.name.clone();
    let occurrence = intent.occurrence()?;

    let outcome = match side.pool.primary() {
        #[cfg(feature = "gcal")]
        Client::Gcal(client) => {
            let target = match &occurrence {
                Some(recurrence_id) => client.occurrence(&collection, &handle, recurrence_id),
                None => Ok(handle.clone()),
            };
            target.and_then(|target| match invitation {
                Invitation::Reply { partstat, comment } => {
                    client.reply(&collection, &target, partstat, comment.as_deref())
                }
                Invitation::Cancel { .. } => client.cancel(&collection, &target),
            })
        }
        #[cfg(feature = "msgraph")]
        Client::Msgraph(client) if client.kind() == GraphKind::Calendar => {
            let target = match &occurrence {
                Some(recurrence_id) => client.occurrence_event(&handle, recurrence_id),
                None => Ok(handle.clone()),
            };
            target.and_then(|target| match invitation {
                Invitation::Reply { partstat, comment } => {
                    client.reply_event(&target, partstat, comment.as_deref())
                }
                Invitation::Cancel { comment } => client.cancel_event(&target, comment.as_deref()),
            })
        }
        #[allow(unreachable_patterns)]
        _ => {
            let _: (&Invitation, &str, &str, &Option<String>) =
                (&invitation, &collection, &handle, &occurrence);
            Err(anyhow::Error::new(Refusal(format!(
                "Source {name} cannot perform {}",
                intent.kind
            ))))
        }
    };

    outcome.map_err(InvitationFailure::classify)
}

/// Acknowledges or fails one performed invitation intent, and reports it.
///
/// False when the outcome could not be recorded in the queue.
pub(super) fn settle_invitation(
    store: &mut PimdirSourceStore,
    intent: &InvitationIntent,
    performer: &str,
    outcome: Result<(), InvitationFailure>,
    report: &mut SyncOutput,
) -> bool {
    let row = QueuedIntent {
        id: intent.id,
        collection: &intent.collection,
        kind: &intent.kind,
        seq: intent.seq(),
    };
    settle_intent(store, row, performer, outcome, report)
}

/// The queue row of an intent neverest performs, as it is settled and
/// reported.
struct QueuedIntent<'a> {
    id: i64,
    collection: &'a str,
    kind: &'a str,
    seq: Option<i64>,
}

/// Acknowledges or fails one performed intent, and reports it; false when
/// the outcome could not be recorded in the queue.
///
/// A performed intent is acknowledged with its receipt, so a producer reads it
/// applied rather than withdrawn (pimdir STORAGE §15.4). The receipt names no
/// item: what was performed arrives with a later listing or stays in place.
fn settle_intent(
    store: &mut PimdirSourceStore,
    intent: QueuedIntent<'_>,
    performer: &str,
    outcome: Result<(), InvitationFailure>,
    report: &mut SyncOutput,
) -> bool {
    let mut recorded = true;
    let (failure, parked) = match outcome {
        Ok(()) => {
            info!(
                "{} intent #{} performed by {performer}",
                intent.kind, intent.id
            );
            match store.acknowledge_action(intent.id, None) {
                Ok(true) => {}
                Ok(false) => warn!(
                    "{} intent #{} vanished from the queue before its acknowledgement",
                    intent.kind, intent.id
                ),
                Err(err) => {
                    recorded = false;
                    warn!(
                        "{} intent #{} was performed but could not be acknowledged: {err}",
                        intent.kind, intent.id
                    );
                }
            }
            (None, false)
        }
        Err(failure) => {
            let error = format!("{:#}", failure.error());
            let parked = failure.parks();
            warn!("{} intent #{} failed: {error}", intent.kind, intent.id);
            if let Err(err) = store.fail_action(intent.id, parked.then_some(error.as_str())) {
                recorded = false;
                warn!(
                    "cannot record {} intent #{} failure: {err}",
                    intent.kind, intent.id
                );
            }
            (Some(Failure::of(failure.error())), parked)
        }
    };
    let (code, error, sources) = Failure::row(failure);

    report.intents.push(IntentEntry {
        id: intent.id,
        collection: intent.collection.to_owned(),
        kind: intent.kind.to_owned(),
        source: performer.to_string(),
        seq: intent.seq,
        code,
        error,
        sources,
        parked,
    });
    recorded
}
