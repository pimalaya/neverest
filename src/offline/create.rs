//! # The collection-create intent
//!
//! The `collection-create` queue intent (pimdir STORAGE Annex B.2): a
//! collection created on a source's server, under a parent or at the top
//! level, rather than written to the store.
//!
//! ```json
//! {"v":1,"source":"imap","name":"Archives","parent":"imap/INBOX"}
//! ```
//!
//! An intent is anchored on its parent, else on a collection of the kind
//! the new one takes. The source it names creates the collection on its
//! server, and the collection arrives with the next listing, the same run's
//! when the intent is performed before it. One its server already lists
//! under that name is success, so a crash between the creation and the
//! acknowledgement performs it again harmlessly.

use anyhow::anyhow;
use io_pimdir::{
    client::PimdirStore,
    codec::PimdirAction,
    intent::{COLLECTION_CREATE, PimdirCollectionCreate},
};

use crate::offline::invitation::Failure;

/// One pending collection-create, as read from the queue.
///
/// The payload stays raw so a malformed one parks with its reason rather
/// than hiding the row.
#[derive(Clone, Debug)]
pub struct CreateIntent {
    /// The queue row's append id, the handle to acknowledge or park it.
    pub id: i64,
    /// The collection it is anchored on.
    pub collection: String,
    /// The raw versioned JSON payload.
    pub payload: String,
}

impl CreateIntent {
    /// What it asks for, a malformed payload parking.
    pub fn request(&self) -> Result<PimdirCollectionCreate, Failure> {
        PimdirCollectionCreate::from_payload(&self.payload).map_err(|err| {
            Failure::Permanent(anyhow!("Malformed {COLLECTION_CREATE} payload: {err}"))
        })
    }

    /// The source the producer named to perform it, best effort.
    pub fn source(&self) -> Option<String> {
        self.request().ok().and_then(|request| request.source)
    }
}

/// Every pending collection-create in the store, in queue order.
///
/// The drain leaves the kinds pimdir does not apply itself (they read back
/// as [`PimdirAction::Unknown`]), so this reads exactly what it skipped.
pub fn pending(store: &PimdirStore) -> anyhow::Result<Vec<CreateIntent>> {
    let rows = store
        .list_pending_actions()
        .map_err(|err| anyhow!("Cannot read the queue: {err}"))?;

    Ok(rows
        .into_iter()
        .filter_map(|row| match row.action {
            PimdirAction::Unknown { kind, payload, .. } if kind == COLLECTION_CREATE => {
                Some(CreateIntent {
                    id: row.id,
                    collection: row.collection,
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

    fn intent(payload: &str) -> CreateIntent {
        CreateIntent {
            id: 1,
            collection: "imap/INBOX".into(),
            payload: payload.into(),
        }
    }

    #[test]
    fn a_create_decodes_its_name_parent_and_performer() {
        let create = intent(r#"{"v":1,"source":"imap","name":"Archives","parent":"imap/INBOX"}"#);

        let request = create.request().unwrap();
        assert_eq!(create.source().as_deref(), Some("imap"));
        assert_eq!(request.name, "Archives");
        assert_eq!(
            request.parent.as_ref().map(|parent| parent.as_str()),
            Some("imap/INBOX")
        );
    }

    #[test]
    fn a_malformed_create_parks() {
        let parks = |payload: &str| intent(payload).request().unwrap_err().parks();

        assert!(parks("{"));
        assert!(parks(r#"{"v":2,"name":"Archives"}"#));
        assert!(parks(r#"{"v":1}"#));
        assert!(parks(r#"{"v":1,"name":"  "}"#));
    }
}
