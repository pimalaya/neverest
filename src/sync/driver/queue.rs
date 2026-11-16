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
pub(crate) fn drain_queues(store: &mut PimdirSourceStore, report: &mut SyncOutput) {
    let before = match store.list_pending_actions() {
        Ok(rows) => rows,
        Err(err) => {
            warn!("cannot list the queued actions: {err}");
            return;
        }
    };
    if before.is_empty() {
        return;
    }

    let drained = match store.drain() {
        Ok(drained) => drained,
        Err(err) => {
            warn!("drain failed, actions stay queued: {err}");
            return;
        }
    };
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
        return;
    }

    let parked: BTreeSet<i64> = store
        .parked_actions()
        .map(|rows| rows.into_iter().map(|row| row.id).collect())
        .unwrap_or_default();
    let pending: BTreeSet<i64> = store
        .list_pending_actions()
        .map(|rows| rows.into_iter().map(|row| row.id).collect())
        .unwrap_or_default();
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
}

/// Performs the queued collection intents (pimdir STORAGE Annex B.2), each
/// by the side it names, and settles their rows.
///
/// It runs before the listing, so a collection created, renamed or deleted
/// here is listed as it now stands by the same run. `paired` is whether the
/// account mirrors its source onto targets: there a rename parks, the pair
/// reading it as a delete and a create, and the store follows a rename or a
/// delete only on an account syncing its source alone, a pair settling the
/// collections both endpoints list itself (pimdir SYNC §5).
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
        // NOTE: an intent names its source (pimdir STORAGE §15.6); one whose
        // payload does not decode names none, and goes to a side of its
        // anchor's namespace, which parks it as malformed.
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
            // NOTE: the source alone held it, and its server no longer does:
            // the collection leaves the store as a pair's leaves it once
            // neither endpoint lists it. A row still queued on it keeps it,
            // the store refusing to drop one.
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

/// Performs one collection intent on `side`'s server.
fn perform_collection_intent(
    side: &mut SourceCtx,
    intent: &CollectionIntent,
    request: &collection::Request,
    paired: bool,
) -> Result<Performed, InvitationFailure> {
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

/// Follows a rename its server performed in the store (pimdir STORAGE Annex
/// B.2): where the id derives from the name (IMAP, Gmail), the collection
/// and every one under it take their new id through `rename_collection`,
/// so their items keep their bindings rather than leaving under the old id
/// and arriving again under the new one; each is named as its next listing
/// names it. A failure is logged: the next listing then reads the renamed
/// collection as a new one.
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
/// Each is performed by the side its payload names, on the item that side
/// binds, through the provider's own verbs. A performed intent is
/// acknowledged; a permanent failure parks the row with its error, a
/// transient one leaves it pending. An intent naming a source this run does
/// not drive waits for that source's own run.
pub(super) fn drain_invitations(
    sides: &mut [&mut SourceCtx],
    store: &mut PimdirSourceStore,
    report: &mut SyncOutput,
) {
    let intents = match invitation::pending(store) {
        Ok(intents) => intents,
        Err(err) => {
            warn!("cannot read the queued calendar intents: {err:#}");
            return;
        }
    };
    if intents.is_empty() {
        return;
    }

    for intent in &intents {
        let located = locate_invitation(store, intent);
        // NOTE: an intent names its source (pimdir STORAGE §15.6); one whose
        // payload does not decode names none, and goes to a side of its
        // collection's namespace, the one binding the item when two do,
        // which parks it as malformed.
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

        let side = &mut *sides[index];
        let outcome = located.and_then(|bindings| perform_invitation(side, intent, &bindings));
        let performer = side.name.clone();
        settle_invitation(store, intent, &performer, outcome, report);
    }
}

/// Every source's binding of the item an intent addresses.
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
        // NOTE: the producer's own add, which the drain stages first.
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
    // NOTE: a pending create holds a provisional handle until its add is
    // accepted, the next push (pimdir SYNC §4).
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

    // NOTE: an occurrence is acted on as its own instance, which the
    // provider's verbs take like any event.
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
            // NOTE: read here too, so a build performing no intent at all
            // still uses what the arms above read.
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
pub(super) fn settle_invitation(
    store: &mut PimdirSourceStore,
    intent: &InvitationIntent,
    performer: &str,
    outcome: Result<(), InvitationFailure>,
    report: &mut SyncOutput,
) {
    let row = QueuedIntent {
        id: intent.id,
        collection: &intent.collection,
        kind: &intent.kind,
        seq: intent.seq(),
    };
    settle_intent(store, row, performer, outcome, report);
}

/// The queue row of an intent neverest performs, as it is settled and
/// reported.
struct QueuedIntent<'a> {
    id: i64,
    collection: &'a str,
    kind: &'a str,
    seq: Option<i64>,
}

/// Acknowledges or fails one performed intent, and reports it.
///
/// A performed intent is acknowledged with its receipt, so a producer reads it
/// applied rather than withdrawn (pimdir STORAGE §15.4). The receipt names no
/// item: a created collection arrives with a later listing, and an invitation
/// answered or cancelled leaves its event where the producer found it.
fn settle_intent(
    store: &mut PimdirSourceStore,
    intent: QueuedIntent<'_>,
    performer: &str,
    outcome: Result<(), InvitationFailure>,
    report: &mut SyncOutput,
) {
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
                Err(err) => warn!(
                    "{} intent #{} was performed but could not be acknowledged, it will be performed again: {err}",
                    intent.kind, intent.id
                ),
            }
            (None, false)
        }
        Err(failure) => {
            let error = format!("{:#}", failure.error());
            let parked = failure.parks();
            warn!("{} intent #{} failed: {error}", intent.kind, intent.id);
            if let Err(err) = store.fail_action(intent.id, parked.then_some(error.as_str())) {
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
}
