//! # Sync hunks
//!
//! The collection, item and flag changes a sync applied, rendered in the
//! printed report and in `--json`.
//!
//! Under io-replica these are descriptors the driver emits per cross-side
//! propagation step, the per-side reconcile being internal and not
//! itemized. `content_key` is the cross-side alignment key, skipped from
//! JSON to keep the report shape stable.

use std::{collections::BTreeSet, fmt};

use schemars::JsonSchema;
use serde::Serialize;

use crate::item::flag::Flag;

/// Collection-level change: create or delete a collection on one side.
///
/// `Delete` is kept for the report and `--json` shape, though collection
/// deletion is not propagated yet.
#[derive(Clone, Debug, Serialize, JsonSchema)]
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
    /// items is known this run.
    ///
    /// Its own kind, because reporting one as a create would say the sync
    /// tried to make a collection it never touched.
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
#[derive(Clone, Debug, Serialize, JsonSchema)]
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
    /// Fetch an item from `side` into the local store, which a dry run
    /// reports as its pull plan and a real run simply hydrates.
    Fetch {
        side: String,
        collection: String,
        id: String,
        #[serde(skip)]
        content_key: u64,
    },
    /// Replace `side`'s copy of the item's body in place.
    ///
    /// Never emitted for mail, whose bodies are immutable.
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

/// Lowercase comma-joined flag list in brackets, e.g. `[\seen, \flagged]`.
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
