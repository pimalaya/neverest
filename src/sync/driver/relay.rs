//! # Relay
//!
//! Cross-copy bodies streamed from one side to the other, never held whole
//! or stored, each copy then bound in the store.

use std::io::pipe;

use crate::sync::driver::*;

/// One cross-copy body to relay.
///
/// `size` is the exact octet length from the item's summary, so the target
/// append is length-prefixed without buffering the body.
pub(super) struct RelayTarget {
    /// The name of the source holding the body.
    pub(super) holding: String,
    handle: PimdirHandle,
    /// The cross-side identity, so a relay is itemized under the hydrating id.
    pub(super) link: String,
    /// The identity the target addresses the new item by.
    ///
    /// Owned, because the link id it points into is dropped with the hub.
    hint: Option<String>,
    /// The minted part of the key, naming a copy apart from its twin.
    mint: Option<String>,
    size: usize,
    /// The markers to append the copy with, as its backend spells them.
    pub(super) flags: Vec<Flag>,
    /// The same markers as the hub holds them, for the recorded placement.
    ///
    /// [`Flag`] equality collapses two wire spellings into one, so a round
    /// trip through it can drop a marker the hub kept.
    marks: PimdirFlags,
    /// The item's cached summary, which the copy shares: the same message
    /// on the other side, so the same summary.
    summary: Option<PimdirSummary>,
}

/// The relay targets: a one-sided, never-hydrated item its far side may create.
///
/// Only into a target whose closed listing covers the item's date (pimdir SYNC
/// §5), so a message it holds already is bound by its listing, not appended
/// twice; a round open or never closed covers nothing.
pub(super) fn relay_targets(
    store: &PimdirStore,
    kind: Kind,
    collection: &str,
    left: (&str, bool),
    right: (&str, bool),
) -> Result<Vec<RelayTarget>> {
    let hub = store.load_hub(collection)?;
    let coverages = store.list_coverage(collection)?;
    let covered = |target: &str| {
        coverages
            .iter()
            .find(|coverage| coverage.source == target && coverage.round.is_none())
            .and_then(|coverage| coverage.coverage.as_ref())
            .map(|coverage| &coverage.scope)
    };

    let mut out = Vec::new();
    for (link, item) in &hub.items {
        if item.deleted || item.object.is_some() || item.sources.len() != 1 {
            continue;
        }
        let (held, binding) = item.sources.iter().next().expect("one source");
        let holding = held.0.clone();
        let (target, target_creates) = if holding == left.0 { right } else { left };
        if !target_creates {
            continue;
        }
        let date = match &item.summary {
            Some(PimdirSummary::Mail(mail)) => mail.date.as_deref(),
            _ => None,
        };
        if !covered(target).is_some_and(|scope| scope.contains(date)) {
            debug!(
                "relay holds {} of {collection} back until {target} lists it",
                link.0
            );
            continue;
        }
        let Some(size) = summary_size(&item.summary) else {
            warn!(
                "relay skips {} in {collection}: no size in its summary",
                link.0
            );
            continue;
        };
        let split = kind.split_link_id(link);
        out.push(RelayTarget {
            holding,
            handle: binding.handle.clone(),
            link: link.0.clone(),
            hint: split.hint.map(str::to_string),
            mint: split.mint.map(str::to_string),
            size,
            flags: to_email_flag_set(&item.flags).into_iter().collect(),
            marks: item.flags.clone(),
            summary: item.summary.clone(),
        });
    }
    Ok(out)
}

/// The octet length a mail summary carries, when present.
///
/// The one kind whose summary records a size (pimdir STORAGE Annex A.1), and
/// the one kind a body crosses by relay.
pub(super) fn summary_size(summary: &Option<PimdirSummary>) -> Option<usize> {
    match summary.as_ref()? {
        PimdirSummary::Mail(mail) => mail.size.map(|size| size as usize),
        _ => None,
    }
}

/// The date a mail summary carries; other kinds carry none.
pub(super) fn summary_date(summary: &Option<PimdirSummary>) -> Option<String> {
    match summary.as_ref()? {
        PimdirSummary::Mail(mail) => mail.date.clone(),
        _ => None,
    }
}

/// Streams each cross-copy body between the sides, keeping only the spine.
///
/// A relay never reaches the projection the hydrating path reports from, so
/// each write is itemized here instead, or a run that relayed would report
/// writing nothing. A copy is recorded against the side that now holds it.
pub(super) fn relay_copies(
    collection: &str,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
    left_store: &mut PimdirSourceStore,
    right_store: &mut PimdirSourceStore,
    progress: &CollectionProgress,
    report: &mut SyncOutput,
) -> Result<usize> {
    let targets = relay_targets(
        left_store,
        resolve_kind(&mut left.pool),
        collection,
        (&left.name, left.perms.item.create),
        (&right.name, right.perms.item.create),
    )?;
    let wire = wire_name(&left.namespace, collection).to_string();
    let total = targets.len();
    let mut count = 0;

    for target in targets {
        let holds_left = target.holding == left.name;
        let (holding_pool, target_pool) = if holds_left {
            (&mut left.pool, &mut right.pool)
        } else {
            (&mut right.pool, &mut left.pool)
        };
        let (target_name, target_store) = if holds_left {
            (right.name.clone(), &mut *right_store)
        } else {
            (left.name.clone(), &mut *left_store)
        };

        let written = relay_one(holding_pool, target_pool, &wire, &target)
            .with_context(|| format!("Relay {} in {wire}", target.handle.0))?;

        bind_relayed(target_store, collection, &target, written)
            .with_context(|| format!("Record the relay of {} in {wire}", target.handle.0))?;

        report.item_patch.push(PatchEntry::new(
            ItemHunk::Copy {
                source_side: target.holding.clone(),
                target_side: target_name,
                collection: wire.clone(),
                source_id: target.link.clone(),
                flags: target.flags.iter().cloned().collect(),
                origin: None,
                content_key: content_key(&target.link),
            },
            None,
        ));
        count += 1;
        progress.tick(count, total);
    }
    Ok(count)
}

/// Records the copy the target holds, bound to the item it was relayed from.
///
/// Around the engine, the relay owes the store the handle under the item's
/// link id, at `Meta` since `Full` would strand a body-less copy, with the
/// accepted append as base, or a later pass relays or appends it again.
pub(super) fn bind_relayed(
    store: &mut PimdirSourceStore,
    collection: &str,
    target: &RelayTarget,
    written: WrittenItem,
) -> Result<()> {
    let placement = PimdirPlacement {
        collection: PimdirCollectionId(collection.to_string()),
        handle: PimdirHandle(written.id),
        link_id: Some(PimdirLinkId(target.link.clone())),
        object: None,
        level: PimdirLevel::Meta,
        summary: target.summary.clone(),
        sort_key: PimdirSortKey::default(),
        flags: target.marks.clone(),
        status: PimdirStatus::Clean,
        conflict_revision: None,
        conflict_object: None,
        base: Some(PimdirBase {
            flags: target.marks.clone(),
            revision: written.revision,
            object: None,
        }),
        origin: None,
    };

    store
        .write(vec![PimdirWriteOp::UpsertPlacement(placement)])
        .map(|_| ())
        .map_err(|err| anyhow!("Storage write error: {err}"))
}

/// Relays one message: a fetch thread into the pipe, this one into the append.
///
/// The body is never held whole or stored; `collection` is the wire name. A
/// copy the server does not name fails, the relay having no create a listing
/// could land.
fn relay_one(
    holding_pool: &mut Pool,
    target_pool: &mut Pool,
    collection: &str,
    target: &RelayTarget,
) -> Result<WrittenItem> {
    let (reader, writer) = pipe().context("relay pipe")?;
    let holding = holding_pool.primary();
    let dest = target_pool.primary();
    let handle = target.handle.0.clone();

    let (fetch, append) = std::thread::scope(|scope| {
        let fetch = scope.spawn(move || holding.get_item_stream(collection, &handle, writer));
        let append = dest.add_item_stream(
            collection,
            &target.flags,
            reader,
            target.size,
            LinkId {
                hint: target.hint.as_deref(),
                mint: target.mint.as_deref(),
            },
        );
        (fetch.join().unwrap(), append)
    });
    fetch.context("relay fetch")?;
    append
        .context("relay append")?
        .ok_or_else(|| anyhow!("Cannot resolve appended UID: the server names no appended message"))
}
