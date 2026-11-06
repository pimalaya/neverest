//! # Sync events
//!
//! What `neverest sync --json` prints while it runs, before its
//! [`SyncOutput`]: one line per event, each told apart from the result by
//! its `event` key. Without `--json`, the spinner on stderr says the same.
//!
//! Events report progress only. What the run did and left (hunks,
//! conflicts, parked work) stays in the result, so a caller reading the
//! last line alone loses nothing.
//!
//! [`SyncOutput`]: crate::sync::report::SyncOutput

use std::fmt;

use schemars::JsonSchema;
use serde::Serialize;

/// One step of a sync run, as soon as the run observes it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SyncEvent {
    /// The run reached an endpoint: its first connection is open.
    Opened {
        /// The endpoint, as the account names it.
        endpoint: String,
    },
    /// An endpoint listed the collections it holds.
    Listed {
        /// The endpoint, as the account names it.
        endpoint: String,
        /// How many collections it listed, filtered or not.
        collections: usize,
    },
    /// A source and a target started reconciling one collection, the
    /// `position`th of the `total` they both hold.
    CollectionStarted {
        /// The endpoint the collection comes from.
        source: String,
        /// The endpoint the collection crosses to.
        target: String,
        /// The collection name.
        collection: String,
        /// The collection's place in the run, from 1.
        position: usize,
        /// How many collections the pair reconciles.
        total: usize,
    },
    /// Items of a collection crossed between a source and a target: `done`
    /// of the `total` the collection has to move. Printed once per percent.
    CollectionProgressed {
        /// The endpoint the collection comes from.
        source: String,
        /// The endpoint the collection crosses to.
        target: String,
        /// The collection name.
        collection: String,
        /// How many items crossed so far.
        done: usize,
        /// How many items the collection has to move.
        total: usize,
    },
    /// A source and a target are done with one collection, `error` naming
    /// why it stopped short. The run goes on with the next collection.
    CollectionFinished {
        /// The endpoint the collection comes from.
        source: String,
        /// The endpoint the collection crosses to.
        target: String,
        /// The collection name.
        collection: String,
        /// Why the collection stopped short, `null` when it did not.
        error: Option<String>,
    },
    /// A source with no target listed `done` of its `total` collections
    /// into the store. Printed as the scan starts, then once per collection.
    ScanProgressed {
        /// The source, as the account names it.
        source: String,
        /// How many collections it listed so far.
        done: usize,
        /// How many collections it lists.
        total: usize,
    },
    /// A source with no target downloaded `done` of the `total` bodies the
    /// store owes. Printed as the download starts, then once per percent.
    DownloadProgressed {
        /// The source, as the account names it.
        source: String,
        /// How many bodies it downloaded so far.
        done: usize,
        /// How many bodies the store owes.
        total: usize,
    },
}

impl fmt::Display for SyncEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Opened { endpoint } => writeln!(f, "Opened {endpoint}"),
            Self::Listed {
                endpoint,
                collections,
            } => writeln!(f, "Listed {collections} collection(s) on {endpoint}"),
            Self::CollectionStarted {
                collection,
                position,
                total,
                ..
            } => writeln!(f, "[{position}/{total}] Syncing {collection}"),
            Self::CollectionProgressed {
                collection,
                done,
                total,
                ..
            } => writeln!(f, "Syncing {collection} ({done}/{total})"),
            Self::CollectionFinished {
                collection,
                error: None,
                ..
            } => writeln!(f, "{collection}: done"),
            Self::CollectionFinished {
                collection,
                error: Some(error),
                ..
            } => writeln!(f, "{collection}: error ({error})"),
            Self::ScanProgressed {
                source,
                done,
                total,
            } => writeln!(f, "Scanning {source} ({done}/{total})"),
            Self::DownloadProgressed {
                source,
                done,
                total,
            } => writeln!(f, "Downloading {source} ({done}/{total})"),
        }
    }
}
