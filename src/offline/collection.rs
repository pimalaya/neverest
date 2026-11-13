//! # The collection intents
//!
//! The queue intents acting on a source's collections on its server rather
//! than in the store (pimdir STORAGE Annex B.2): `collection-create`,
//! `collection-rename` and `collection-delete`.
//!
//! ```json
//! {"v":1,"source":"imap","name":"Archives","parent":"imap/INBOX"}
//! {"v":1,"source":"imap","name":"Old projects"}
//! {"v":1,"source":"imap"}
//! ```
//!
//! A creation is anchored on its parent, else on a collection of the kind
//! the new one takes; a rename and a delete on the collection they act on,
//! which their payload does not restate. The source an intent names
//! performs it on its server, and the result arrives with the next listing,
//! the same run's when the intent is performed before it. Each is at least
//! once: a server already holding what was asked is success, so a crash
//! between the request and the acknowledgement performs it again harmlessly.

use anyhow::anyhow;
use io_pimdir::{
    client::PimdirStore,
    codec::PimdirAction,
    intent::{
        COLLECTION_CREATE, COLLECTION_DELETE, COLLECTION_RENAME, PimdirCollectionCreate,
        PimdirCollectionDelete, PimdirCollectionRename,
    },
};

use crate::offline::{invitation::Failure, states_unknown_version};

/// What a collection intent asks for, decoded.
#[derive(Clone, Debug)]
pub enum Request {
    /// A collection created under a parent or at the top level.
    Create(PimdirCollectionCreate),
    /// The anchor renamed, its parent kept.
    Rename(PimdirCollectionRename),
    /// The anchor deleted, with the items it holds.
    Delete(PimdirCollectionDelete),
}

impl Request {
    /// The source the producer named to perform it.
    pub fn source(&self) -> &str {
        match self {
            Self::Create(request) => &request.source,
            Self::Rename(request) => &request.source,
            Self::Delete(request) => &request.source,
        }
    }
}

/// One pending collection intent, as read from the queue.
///
/// The payload stays raw so a malformed one parks with its reason rather
/// than hiding the row.
#[derive(Clone, Debug)]
pub struct CollectionIntent {
    /// The queue row's append id, the handle to acknowledge or park it.
    pub id: i64,
    /// The collection it is anchored on.
    pub collection: String,
    /// Its kind: [`COLLECTION_CREATE`], [`COLLECTION_RENAME`] or
    /// [`COLLECTION_DELETE`].
    pub kind: &'static str,
    /// The raw versioned JSON payload.
    pub payload: String,
}

impl CollectionIntent {
    /// What it asks for, a malformed payload parking.
    pub fn request(&self) -> Result<Request, Failure> {
        let malformed = |err: &dyn std::fmt::Display| {
            Failure::Permanent(anyhow!("Malformed {} payload: {err}", self.kind))
        };

        match self.kind {
            COLLECTION_CREATE => PimdirCollectionCreate::from_payload(&self.payload)
                .map(Request::Create)
                .map_err(|err| malformed(&err)),
            COLLECTION_RENAME => PimdirCollectionRename::from_payload(&self.payload)
                .map(Request::Rename)
                .map_err(|err| malformed(&err)),
            _ => PimdirCollectionDelete::from_payload(&self.payload)
                .map(Request::Delete)
                .map_err(|err| malformed(&err)),
        }
    }

    /// The source the producer named to perform it, best effort.
    pub fn source(&self) -> Option<String> {
        self.request()
            .ok()
            .map(|request| request.source().to_owned())
    }
}

/// The kind of a collection intent, as the queue spells it.
fn collection_kind(kind: &str) -> Option<&'static str> {
    [COLLECTION_CREATE, COLLECTION_RENAME, COLLECTION_DELETE]
        .into_iter()
        .find(|known| *known == kind)
}

/// Every pending collection intent in the store, in queue order.
///
/// The drain leaves the kinds pimdir does not apply itself (they read back
/// as [`PimdirAction::Unknown`]), so this reads exactly what it skipped.
/// One stating a version this neverest does not know is left out, so it
/// stays pending and untouched (pimdir STORAGE §15.3).
pub fn pending(store: &PimdirStore) -> anyhow::Result<Vec<CollectionIntent>> {
    let rows = store
        .list_pending_actions()
        .map_err(|err| anyhow!("Cannot read the queue: {err}"))?;

    Ok(rows
        .into_iter()
        .filter_map(|row| match row.action {
            PimdirAction::Unknown { kind, payload, .. } => {
                let kind = collection_kind(&kind)?;
                (!states_unknown_version(kind, &payload)).then_some(CollectionIntent {
                    id: row.id,
                    collection: row.collection,
                    kind,
                    payload,
                })
            }
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(kind: &'static str, payload: &str) -> CollectionIntent {
        CollectionIntent {
            id: 1,
            collection: "imap/INBOX".into(),
            kind,
            payload: payload.into(),
        }
    }

    #[test]
    fn a_create_decodes_its_name_parent_and_performer() {
        let create = intent(
            COLLECTION_CREATE,
            r#"{"v":1,"source":"imap","name":"Archives","parent":"imap/INBOX"}"#,
        );

        let Ok(Request::Create(request)) = create.request() else {
            panic!("a create decodes as one");
        };
        assert_eq!(create.source().as_deref(), Some("imap"));
        assert_eq!(request.name, "Archives");
        assert_eq!(
            request.parent.as_ref().map(|parent| parent.as_str()),
            Some("imap/INBOX")
        );
    }

    #[test]
    fn a_rename_and_a_delete_decode_their_performer() {
        let rename = intent(COLLECTION_RENAME, r#"{"v":1,"source":"imap","name":"Old"}"#);
        let delete = intent(COLLECTION_DELETE, r#"{"v":1,"source":"gmail"}"#);

        let Ok(Request::Rename(request)) = rename.request() else {
            panic!("a rename decodes as one");
        };
        assert_eq!(request.name, "Old");
        assert_eq!(rename.source().as_deref(), Some("imap"));
        assert!(matches!(delete.request(), Ok(Request::Delete(_))));
        assert_eq!(delete.source().as_deref(), Some("gmail"));
    }

    #[test]
    fn a_malformed_intent_parks() {
        let parks = |kind, payload: &str| intent(kind, payload).request().unwrap_err().parks();

        assert!(parks(COLLECTION_CREATE, "{"));
        assert!(parks(COLLECTION_CREATE, r#"{"name":"Archives"}"#));
        assert!(parks(COLLECTION_CREATE, r#"{"v":1}"#));
        assert!(parks(COLLECTION_CREATE, r#"{"v":1,"name":"  "}"#));
        assert!(parks(COLLECTION_RENAME, r#"{"v":1,"source":"imap"}"#));
        assert!(parks(
            COLLECTION_RENAME,
            r#"{"v":1,"source":"imap","name":"Old","collection":"imap/A"}"#
        ));
        assert!(parks(COLLECTION_DELETE, r#"{"v":1}"#));
    }

    #[test]
    fn only_the_collection_kinds_are_read() {
        assert_eq!(
            collection_kind("collection-rename"),
            Some(COLLECTION_RENAME)
        );
        assert_eq!(
            collection_kind("collection-delete"),
            Some(COLLECTION_DELETE)
        );
        assert_eq!(collection_kind("submit"), None);
    }
}
