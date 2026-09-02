//! # Sync hunks
//!
//! The collection, item and flag changes a sync applied, rendered in the
//! printed report and in `--json`.
//!
//! Under io-replica these are descriptors the driver emits per cross-side
//! propagation step, plus the flag changes and removals its opening pull-only
//! round found a server had made. `content_key` is the cross-side alignment
//! key, skipped from JSON to keep the report shape stable.

use std::{collections::BTreeSet, fmt};

use schemars::JsonSchema;
use serde::Serialize;

use crate::item::flag::Flag;

/// Collection-level change: create or delete a collection on one side.
///
/// `Delete` is kept for the report and `--json` shape, though collection
/// deletion is not propagated yet.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
#[allow(dead_code)]
pub enum CollectionHunk {
    /// Create the collection on `side`.
    Create {
        /// The endpoint the change is applied on.
        side: String,
        /// The collection, by the name its server answers to.
        collection: String,
    },
    /// Delete `side`'s copy of the collection.
    Delete {
        /// The endpoint the change is applied on.
        side: String,
        /// The collection, by the name its server answers to.
        collection: String,
    },
    /// The collection could not be reconciled at all, so nothing about its
    /// items is known this run.
    ///
    /// Its own kind, because reporting one as a create would say the sync
    /// tried to make a collection it never touched.
    Scan {
        /// The endpoint whose collection could not be read.
        side: String,
        /// The collection, by the name its server answers to.
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
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ItemHunk {
    /// Copy an item from `source_side` to `target_side`.
    Copy {
        /// The endpoint the body is read from.
        source_side: String,
        /// The endpoint it is written to.
        target_side: String,
        /// The collection, by the name its server answers to.
        collection: String,
        /// The item's link id, which both sides know it by.
        source_id: String,
        /// The flags the copy is written with.
        flags: BTreeSet<Flag>,
        /// The cross-side alignment key, never serialized.
        #[serde(skip)]
        content_key: u64,
    },
    /// Add `flags` on `side`'s copy of the item.
    AddFlags {
        /// The endpoint the change is applied on.
        side: String,
        /// The collection, by the name its server answers to.
        collection: String,
        /// The item's handle on that endpoint.
        id: String,
        /// The flags being set.
        flags: BTreeSet<Flag>,
        /// The cross-side alignment key, never serialized.
        #[serde(skip)]
        content_key: u64,
    },
    /// Remove `flags` from `side`'s copy of the item.
    RemoveFlags {
        /// The endpoint the change is applied on.
        side: String,
        /// The collection, by the name its server answers to.
        collection: String,
        /// The item's handle on that endpoint.
        id: String,
        /// The flags being cleared.
        flags: BTreeSet<Flag>,
        /// The cross-side alignment key, never serialized.
        #[serde(skip)]
        content_key: u64,
    },
    /// Delete `side`'s copy of the item.
    Delete {
        /// The endpoint the change is applied on.
        side: String,
        /// The collection, by the name its server answers to.
        collection: String,
        /// The item's handle on that endpoint.
        id: String,
        /// The cross-side alignment key, never serialized.
        #[serde(skip)]
        content_key: u64,
    },
    /// Fetch an item from `side` into the local store, which a dry run
    /// reports as its pull plan and a real run simply hydrates.
    Fetch {
        /// The endpoint the body is read from.
        side: String,
        /// The collection, by the name its server answers to.
        collection: String,
        /// The item's handle on that endpoint.
        id: String,
        /// The cross-side alignment key, never serialized.
        #[serde(skip)]
        content_key: u64,
    },
    /// Replace `side`'s copy of the item's body in place.
    ///
    /// Never emitted for mail, whose bodies are immutable.
    Update {
        /// The endpoint the change is applied on.
        side: String,
        /// The collection, by the name its server answers to.
        collection: String,
        /// The item's handle on that endpoint.
        id: String,
        /// The cross-side alignment key, never serialized.
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
