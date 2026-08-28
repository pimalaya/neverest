//! Report DTOs: the collection / item / flag changes a sync applied, rendered
//! in the printed report and `--json`.
//!
//! These were the applied work units of the v1 engine; under io-replica they
//! are pure descriptors the driver emits for each cross-side propagation step
//! (the per-side server reconcile is internal and not itemized). `content_key`
//! is the cross-side alignment key, skipped from JSON to keep the report shape
//! stable.

use std::{collections::BTreeSet, fmt};

use serde::Serialize;

use crate::item::flag::Flag;

/// Collection-level change: create or delete a collection on one side.
///
/// `Delete` is kept for the report and --json shape, though collection
/// deletion is not propagated yet (only creation).
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[allow(dead_code)]
pub enum CollectionHunk {
    Create {
        side: String,
        collection: String,
    },
    Delete {
        side: String,
        collection: String,
    },
    /// The collection could not be reconciled at all, so nothing about its
    /// items is known this run. Carried as its own kind because reusing a
    /// create to report one says the sync tried to make a collection it never
    /// touched.
    Scan {
        side: String,
        collection: String,
    },
}

impl fmt::Display for CollectionHunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create { side, collection } => {
                write!(f, "create collection {collection} on {side}")
            }
            Self::Delete { side, collection } => {
                write!(f, "delete collection {collection} on {side}")
            }
            Self::Scan { side, collection } => {
                write!(f, "scan collection {collection} on {side}")
            }
        }
    }
}

/// Item-level change.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ItemHunk {
    /// Copy an item from `source_side` to `target_side`.
    Copy {
        source_side: String,
        target_side: String,
        collection: String,
        source_id: String,
        flags: BTreeSet<Flag>,
        #[serde(skip)]
        content_key: u64,
    },
    /// Add `flags` on `side`'s copy of the item.
    AddFlags {
        side: String,
        collection: String,
        id: String,
        flags: BTreeSet<Flag>,
        #[serde(skip)]
        content_key: u64,
    },
    /// Remove `flags` from `side`'s copy of the item.
    RemoveFlags {
        side: String,
        collection: String,
        id: String,
        flags: BTreeSet<Flag>,
        #[serde(skip)]
        content_key: u64,
    },
    /// Delete `side`'s copy of the item.
    Delete {
        side: String,
        collection: String,
        id: String,
        #[serde(skip)]
        content_key: u64,
    },
    /// Fetch (download) an item from `side` into the local store — the pull
    /// plan of a one-source local sync (reported by a dry run; a real run just
    /// hydrates it).
    Fetch {
        side: String,
        collection: String,
        id: String,
        #[serde(skip)]
        content_key: u64,
    },
    /// Replace `side`'s copy of the item's body in place — a mutable-content
    /// edit. Never emitted for mail, whose bodies are immutable.
    Update {
        side: String,
        collection: String,
        id: String,
        #[serde(skip)]
        content_key: u64,
    },
}

impl fmt::Display for ItemHunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Copy {
                source_side,
                target_side,
                collection,
                source_id,
                ..
            } => write!(
                f,
                "copy item {source_id} in {collection} from {source_side} to {target_side}"
            ),
            Self::AddFlags {
                side,
                collection,
                id,
                flags,
                ..
            } => write!(
                f,
                "add {flags} to item {id} in {collection} on {side}",
                flags = format_flag_list(flags),
            ),
            Self::RemoveFlags {
                side,
                collection,
                id,
                flags,
                ..
            } => write!(
                f,
                "remove {flags} from item {id} in {collection} on {side}",
                flags = format_flag_list(flags),
            ),
            Self::Delete {
                side,
                collection,
                id,
                ..
            } => {
                write!(f, "delete item {id} in {collection} on {side}")
            }
            Self::Fetch {
                side,
                collection,
                id,
                ..
            } => {
                write!(f, "fetch item {id} in {collection} from {side}")
            }
            Self::Update {
                side,
                collection,
                id,
                ..
            } => {
                write!(f, "update item {id} in {collection} on {side}")
            }
        }
    }
}

/// Lowercase comma-joined flag list wrapped in brackets, e.g.
/// `[\seen, \flagged]`.
fn format_flag_list(flags: &BTreeSet<Flag>) -> String {
    let mut out = String::from("[");
    let mut first = true;
    for flag in flags {
        if !first {
            out.push_str(", ");
        }
        first = false;
        for ch in flag.raw().chars() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        }
    }
    out.push(']');
    out
}
