//! # Sync events
//!
//! What `neverest sync --json` prints while it runs, before its
//! [`SyncOutput`]: one line per event, each told apart from the result by
//! its `event` key. Without `--json`, the same events print as plain lines
//! on stdout, a pair's per-percent count left out ([`SyncEvent::is_coarse`]).
//!
//! Events report progress, and the refusals a source holds (`refused`),
//! printed as an account ends. What the run did and left (hunks,
//! conflicts, parked work, those refusals) stays in the result, so a
//! caller reading the last line alone loses nothing.
//!
//! [`SyncOutput`]: crate::sync::report::SyncOutput

use std::fmt;

use schemars::JsonSchema;
use serde::Serialize;

use crate::error::{ErrorCode, chain};

/// One step of a sync run, as soon as the run observes it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum SyncEvent {
    /// The run started on an account, the `position`th of the `total` it
    /// covers: the events after it are that account's.
    AccountStarted {
        /// The account, as the configuration names it.
        account: String,
        /// The account's place in the run, from 1.
        position: usize,
        /// How many accounts the run covers.
        total: usize,
    },
    /// The run reached an endpoint: its first connection is open.
    EndpointOpened {
        /// The endpoint, as the account names it.
        endpoint: String,
    },
    /// An endpoint listed the collections it holds.
    EndpointListed {
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
        /// The stable code of why the collection stopped short, `null`
        /// when it did not or no caller is expected to act on it.
        code: Option<ErrorCode>,
        /// Why the collection stopped short, the outermost message, `null`
        /// when it did not.
        error: Option<String>,
        /// The messages of its causes, outermost first.
        sources: Vec<String>,
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
    /// A source refused a change as it stands, this run or a run before.
    ///
    /// The store holds it while the refusal stands (pimdir SYNC §5). Printed
    /// once the account's run ends, before its result, which names it again
    /// under `refusals`.
    ChangeRefused {
        /// The source that refused the change, as the account names it.
        source: String,
        /// The collection the item sits in, as the source names it.
        collection: String,
        /// The item's handle on that source, the item's key for a create.
        id: String,
        /// What the source said.
        reason: String,
    },
}

impl SyncEvent {
    /// Whether text mode prints it: every event but a pair's per-percent
    /// count, a long download reading as it goes.
    pub fn is_coarse(&self) -> bool {
        !matches!(self, Self::CollectionProgressed { .. })
    }

    /// Names its endpoints as `account`'s configuration does, rather than
    /// by the source id the store keys them under, `<account>/<name>`.
    pub fn name_endpoints(&mut self, account: &str) {
        let prefix = format!("{account}/");
        let endpoints = match self {
            Self::AccountStarted { .. } => vec![],
            Self::EndpointOpened { endpoint } | Self::EndpointListed { endpoint, .. } => {
                vec![endpoint]
            }
            Self::CollectionStarted { source, target, .. }
            | Self::CollectionProgressed { source, target, .. }
            | Self::CollectionFinished { source, target, .. } => vec![source, target],
            Self::ScanProgressed { source, .. }
            | Self::DownloadProgressed { source, .. }
            | Self::ChangeRefused { source, .. } => {
                vec![source]
            }
        };

        for endpoint in endpoints {
            if let Some(name) = endpoint.strip_prefix(&prefix) {
                *endpoint = name.to_owned();
            }
        }
    }
}

impl fmt::Display for SyncEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccountStarted {
                account,
                position,
                total,
            } => writeln!(f, "[{position}/{total}] Syncing account {account}"),
            Self::EndpointOpened { endpoint } => writeln!(f, "Opened {endpoint}"),
            Self::EndpointListed {
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
                sources,
                ..
            } => writeln!(f, "{collection}: error ({})", chain(error, sources)),
            Self::ScanProgressed {
                source,
                done,
                total,
            } => writeln!(f, "Scanning {source} ({done}/{total})"),
            Self::DownloadProgressed {
                source,
                done,
                total,
            } => {
                let percent = (done * 100).checked_div(*total).unwrap_or(100);
                writeln!(f, "Downloading {source} {percent}% ({done}/{total})")
            }
            Self::ChangeRefused {
                source,
                collection,
                id,
                reason,
            } => writeln!(f, "{source} refused {id} in {collection}: {reason}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use serde_json::json;

    use crate::{
        error::{ErrorCode, Failure},
        sync::event::SyncEvent,
    };

    /// A collection that stopped short names its failure as an item error
    /// does, code and causes apart, so a caller acts on the code.
    #[test]
    fn a_collection_stopped_short_carries_its_code_and_causes() {
        let err = anyhow!("403 Forbidden").context("Cannot list INBOX");
        let Failure { error, sources, .. } = Failure::of(&err);
        let event = SyncEvent::CollectionFinished {
            source: "imap".into(),
            target: "store".into(),
            collection: "INBOX".into(),
            code: Some(ErrorCode::PermissionDenied),
            error: Some(error),
            sources,
        };

        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({
                "event": "collection-finished",
                "source": "imap",
                "target": "store",
                "collection": "INBOX",
                "code": "permission-denied",
                "error": "Cannot list INBOX",
                "sources": ["403 Forbidden"],
            }),
        );
        assert_eq!(
            event.to_string(),
            "INBOX: error (Cannot list INBOX: 403 Forbidden)\n"
        );
    }

    /// A refusal a source holds is named as its result row names it, the
    /// source as the account names it, under the kebab-case `refused`.
    #[test]
    fn a_refusal_is_an_event_named_as_its_row() {
        let mut event = SyncEvent::ChangeRefused {
            source: "work/msgraph".into(),
            collection: "Archive".into(),
            id: "m1@x".into(),
            reason: "Graph appends to Drafts only".into(),
        };
        event.name_endpoints("work");

        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({
                "event": "change-refused",
                "source": "msgraph",
                "collection": "Archive",
                "id": "m1@x",
                "reason": "Graph appends to Drafts only",
            }),
        );
        assert_eq!(
            event.to_string(),
            "msgraph refused m1@x in Archive: Graph appends to Drafts only\n"
        );
    }
}
