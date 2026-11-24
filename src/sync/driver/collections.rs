//! # Collections
//!
//! The collections each side lists: filtered, diffed between the sides and
//! declared to the store with their names and roles.

use crate::sync::driver::*;

/// The context of the named source in a pair.
fn source_ctx<'a>(
    name: &str,
    left: &'a mut SourceCtx,
    right: &'a mut SourceCtx,
) -> &'a mut SourceCtx {
    if left.name == name { left } else { right }
}

/// A source's collections: the id addressing one, against the name it goes by.
///
/// The two coincide on IMAP and Graph and part ways on DAV, where a segment is
/// routinely a UUID and the name is a `DAV:displayname`. Every hub id and wire
/// call is built from the key; nothing reads the value back.
pub(super) fn names_by_id(listed: &[Collection]) -> BTreeMap<String, String> {
    listed
        .iter()
        .map(|collection| (collection.id.clone(), collection.name.clone()))
        .collect()
}

/// Records what a collection is called, the id staying what addresses it.
///
/// Sources are read in declared order, the first with something to say
/// winning; one reporting no name leaves the bare id. Nothing keys on the
/// column, so a failure costs a label and never a sync, and is logged.
pub(super) fn declare_name(
    store: &PimdirSourceStore,
    collection: &str,
    id: &str,
    listed: &[&BTreeMap<String, String>],
) {
    let name = listed
        .iter()
        .find_map(|names| {
            names
                .get(id)
                .map(String::as_str)
                .filter(|name| !name.trim().is_empty())
        })
        .unwrap_or(id);

    if let Err(err) = store.set_collection_name(collection, name) {
        warn!("cannot name {collection} `{name}`: {err}");
    }
}

/// The role each collection of `listed` states, by id.
///
/// A listing carrying none takes [`Client::lookup_roles`], asked again only
/// once the source's collection set moved (kept in the sidecar). `None` when
/// a lookup failed: the recorded roles stay rather than being cleared.
pub(super) fn stated_roles(
    lookup: impl FnOnce() -> Result<Option<BTreeMap<String, String>>>,
    source: &str,
    listed: &[Collection],
    state: &mut AccountState,
) -> Option<BTreeMap<String, String>> {
    let mut roles: BTreeMap<String, String> = listed
        .iter()
        .filter_map(|c| Some((c.id.clone(), c.role.clone()?)))
        .collect();

    let mut listing: Vec<String> = listed.iter().map(|c| c.id.clone()).collect();
    listing.sort();

    let looked_up = match state.looked_up_roles.get(source) {
        Some(cached) if cached.listing == listing => cached.roles.clone(),
        _ => match lookup() {
            Ok(None) => BTreeMap::new(),
            Ok(Some(found)) => {
                state.looked_up_roles.insert(
                    source.to_owned(),
                    LookedUpRoles {
                        listing,
                        roles: found.clone(),
                    },
                );
                found
            }
            Err(err) => {
                warn!("cannot look up the roles {source} states: {err:#}");
                return None;
            }
        },
    };

    roles.extend(looked_up);
    Some(roles)
}

/// The hub ids, in `namespace`, of the collections of `listed` whose server
/// states they take no tasks.
pub(super) fn taskless_ids(namespace: &str, listed: &[Collection]) -> BTreeSet<String> {
    listed
        .iter()
        .filter(|collection| collection.takes_tasks == Some(false))
        .map(|collection| hub_id(namespace, &collection.id))
        .collect()
}

/// Records `taskless`, hub ids, as the calendars the source's listings
/// stated take no tasks, in place of what its previous listings stated.
pub(super) fn record_taskless(state: &mut AccountState, source: &str, taskless: &BTreeSet<String>) {
    if taskless.is_empty() {
        state.taskless.remove(source);
    } else {
        state.taskless.insert(source.to_owned(), taskless.clone());
    }
}

/// Records the role a source states for a collection, or clears it.
///
/// Only from the source, never from a name; nothing keys on it, so a failure
/// costs a role and never a sync, and is logged.
pub(super) fn declare_role(store: &PimdirSourceStore, collection: &str, role: Option<&str>) {
    if let Err(err) = store.set_collection_role(collection, role) {
        warn!("cannot record the role of {collection} as {role:?}: {err}");
    }
}

/// The ids the filter keeps, matched on the id: a DAV collection is filtered
/// by its path segment, which is what a configuration names.
pub(super) fn filter_collections(
    collections: &BTreeMap<String, String>,
    filter: &CollectionFilter,
) -> BTreeSet<String> {
    let matches = |id: &str, list: &[String]| list.iter().any(|f| f.eq_ignore_ascii_case(id));
    collections
        .keys()
        .filter(|id| match filter {
            CollectionFilter::All => true,
            CollectionFilter::Include(list) => matches(id, list),
            CollectionFilter::Exclude(list) => !matches(id, list),
        })
        .cloned()
        .collect()
}

/// One endpoint as the collection diff reads it: its name and what it lets
/// a run do to its collection set.
type DiffSide<'a> = (&'a str, &'a CollectionPermissions);

/// Three-way collection diff, gated by each side's permissions.
///
/// A collection one side lists is created on the other, unless the store
/// knows both held it (`held_by_both`, the base): then the delete crosses
/// instead, or, where forbidden, leaves it there, never created again.
pub(super) fn diff_collections(
    left: &BTreeSet<String>,
    right: &BTreeSet<String>,
    held_by_both: &BTreeSet<String>,
    (left_name, left_perms): DiffSide<'_>,
    (right_name, right_perms): DiffSide<'_>,
) -> Vec<CollectionHunk> {
    let mut hunks = Vec::new();
    let sides = [
        (
            left.difference(right),
            (left_name, left_perms),
            right_name,
            right_perms,
        ),
        (
            right.difference(left),
            (right_name, right_perms),
            left_name,
            left_perms,
        ),
    ];
    for (only, (here, here_perms), there, there_perms) in sides {
        for collection in only {
            let hunk = match held_by_both.contains(collection) {
                true if here_perms.delete => CollectionHunk::Delete {
                    side: here.to_owned(),
                    collection: collection.clone(),
                },
                true => {
                    info!("{collection} was deleted on {there}, which {here} does not allow");
                    continue;
                }
                false if there_perms.create => CollectionHunk::Create {
                    side: there.to_owned(),
                    collection: collection.clone(),
                },
                false => continue,
            };
            hunks.push(hunk);
        }
    }
    hunks
}

/// The listed collections that the store knows both endpoints held.
///
/// The base of the three-way collection diff, read from the sources holding
/// a binding or a checkpoint in each (pimdir SYNC §5).
pub(super) fn held_by_both<'a>(
    store: &PimdirSourceStore,
    namespace: &str,
    names: impl IntoIterator<Item = &'a String>,
    left: &str,
    right: &str,
) -> Result<BTreeSet<String>> {
    let mut held = BTreeSet::new();
    for name in names {
        let collection = hub_id(namespace, name);
        let sources = store
            .collection_sources(&collection)
            .with_context(|| format!("List the sources of {collection}"))?;
        if sources.iter().any(|source| source == left) && sources.iter().any(|s| s == right) {
            held.insert(name.clone());
        }
    }
    Ok(held)
}

pub(super) fn apply_collection_hunk(
    hunk: &CollectionHunk,
    left: &mut SourceCtx,
    right: &mut SourceCtx,
) -> Result<()> {
    match hunk {
        CollectionHunk::Create { side, collection } => {
            source_ctx(side, left, right)
                .pool
                .primary()
                .create_collection(collection)
                .with_context(|| format!("Create collection {collection} on {side}"))?;
        }
        CollectionHunk::Scan { .. } | CollectionHunk::Sync { .. } => {
            unreachable!("a failure hunk is reported and never applied")
        }
        CollectionHunk::Fetch { .. } | CollectionHunk::Rename { .. } => {
            unreachable!("a fetch or a performed rename is reported, never a pair's to apply")
        }
        CollectionHunk::Delete { side, collection } => {
            source_ctx(side, left, right)
                .pool
                .primary()
                .delete_collection(collection)
                .with_context(|| format!("Delete collection {collection} on {side}"))?;
        }
    }
    Ok(())
}
