//! # Graph contacts
//!
//! The contacts half of [`GraphClient`]: contact folders as collections,
//! contacts as vCards through io-msgraph's `vcard` projection.
//!
//! The delta round lists handles and `changeKey`s only. Graph returns a
//! contact's extended properties, the stash carrying its UID and every line
//! Graph has no slot for, only through `$expand`, which a body read always
//! carries ([`MSGRAPH_CONTACT_STASH_EXPAND`]); a delta row is never
//! projected, so a card never loses its identity to a trimmed row.
//!
//! Graph has no conditional write for contacts, so an `If-Match` from the
//! engine is checked against the server's current `changeKey` right before
//! the write: a contact edited on Graph since the last sync is refused, the
//! next run enumerating the edit, as a DAV server's 412 would make it.

use std::io::{Read, Write};

use anyhow::{Context, Result, bail};
use io_msgraph::v1::rest::users::{
    contact_folders::{MsgraphContactFolder, list::MsgraphContactFoldersListParams},
    contacts::{
        MsgraphContact,
        delta::{MsgraphContactDelta, MsgraphContactsDeltaResponse},
        vcard::MSGRAPH_CONTACT_STASH_EXPAND,
    },
};
use log::{debug, trace, warn};

use super::{GraphClient, decode_checkpoint, encode_checkpoint, is_expired_link};
use crate::{
    client::{EnumEntry, Enumeration, WrittenItem},
    item::collection::Collection,
};

/// The collection id of the default Contacts folder, which the folders
/// endpoint does not list and Graph addresses by omitting the folder segment.
pub const CONTACTS_FOLDER: &str = "contacts";

/// The `$select` of the contacts delta: the handle and the revision, a body
/// being read separately with its stash expanded.
const DELTA_SELECT: &str = "id,changeKey";

/// The page size requested when listing contact folders.
const FOLDER_PAGE_SIZE: u32 = 100;

impl GraphClient {
    /// Lists the default Contacts folder and every top-level contact folder
    /// with its children, keyed by folder id, named by display name.
    pub(super) fn list_contact_folders(&mut self) -> Result<Vec<Collection>> {
        debug!("begin graph contact folder listing");

        let params = MsgraphContactFoldersListParams {
            top: Some(FOLDER_PAGE_SIZE),
            ..Default::default()
        };

        let mut collections = vec![Collection {
            id: CONTACTS_FOLDER.to_owned(),
            name: String::from("Contacts"),
            total: None,
            unread: None,
            role: Some(String::from("default")),
        }];

        let top = self
            .op(|client| client.contact_folders_list(&params))
            .context("List contact folders error")?
            .value;

        for folder in &top {
            push_folder(&mut collections, folder);
            let children = self
                .op(|client| client.contact_child_folders_list(&folder.id, &params))
                .with_context(|| format!("List child folders of {} error", folder.display_name))?
                .value;
            for child in &children {
                push_folder(&mut collections, child);
            }
        }

        debug!("end of graph contact folder listing");
        trace!("contact folders: {}", collections.len());
        Ok(collections)
    }

    /// Enumerates a contact folder through one delta round: handles and
    /// `changeKey`s, the removals of a resumed round as vanished.
    pub(super) fn enumerate_contacts(
        &mut self,
        collection: &str,
        cursor: Option<&[u8]>,
    ) -> Result<Enumeration> {
        let link = cursor.and_then(decode_checkpoint);
        let (rows, fresh, delta_link) = self.contacts_delta_round(collection, link)?;

        let mut items = Vec::new();
        let mut vanished = Vec::new();
        for row in rows {
            if row.contact.id.is_empty() {
                continue;
            }
            if row.removed.is_some() {
                vanished.push(row.contact.id);
            } else {
                items.push(EnumEntry {
                    id: row.contact.id,
                    flags: Default::default(),
                    revision: row.contact.change_key,
                });
            }
        }

        Ok(Enumeration {
            items,
            vanished,
            complete: fresh,
            checkpoint: encode_checkpoint(&delta_link),
        })
    }

    /// Runs one contacts delta round, resuming from the saved link and
    /// falling back to a fresh round on an expired one (HTTP 410).
    fn contacts_delta_round(
        &mut self,
        collection: &str,
        link: Option<String>,
    ) -> Result<(Vec<MsgraphContactDelta>, bool, String)> {
        debug!("begin graph contacts delta round");
        trace!("collection: {collection}, resumed: {}", link.is_some());

        let folder = folder(collection).map(str::to_owned);
        let fresh_round = |client: &mut Self| {
            client
                .op(|graph| graph.contacts_delta(folder.as_deref(), Some(DELTA_SELECT)))
                .with_context(|| format!("Start contacts delta of {collection} error"))
        };

        let mut fresh = link.is_none();
        let mut page = match link {
            None => fresh_round(self)?,
            Some(link) => match self.op(|graph| graph.contacts_delta_from_link(&link)) {
                Ok(page) => page,
                Err(err) if is_expired_link(&err) => {
                    warn!("graph contacts delta link of {collection} expired, restarting");
                    fresh = true;
                    fresh_round(self)?
                }
                Err(err) => {
                    return Err(anyhow::Error::new(err)
                        .context(format!("Resume contacts delta of {collection} error")));
                }
            },
        };

        let mut rows = Vec::new();
        loop {
            let MsgraphContactsDeltaResponse {
                value,
                next_link,
                delta_link,
            } = page;
            rows.extend(value);

            if let Some(delta) = delta_link {
                debug!("end of graph contacts delta round");
                trace!("rows: {}", rows.len());
                return Ok((rows, fresh, delta));
            }
            let next = next_link.with_context(|| {
                format!("Contacts delta page of {collection} carries no paging link")
            })?;
            page = self
                .op(|graph| graph.contacts_delta_from_link(&next))
                .with_context(|| format!("Page contacts delta of {collection} error"))?;
        }
    }

    /// Reads one contact with its stash, the only read a projection trusts.
    fn contact(&mut self, id: &str) -> Result<MsgraphContact> {
        self.op(|graph| graph.contact_get(id, Some(MSGRAPH_CONTACT_STASH_EXPAND)))
            .with_context(|| format!("Get contact {id} error"))
    }

    /// Streams the vCards of an id set, one read per contact, each committed
    /// with the `changeKey` it was projected from.
    pub(super) fn fetch_contacts<S: Write>(
        &mut self,
        ids: &[&str],
        mut open: impl FnMut(&str) -> std::io::Result<S>,
        mut done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<()> {
        for id in ids {
            let contact = self.contact(id)?;
            let mut sink = open(id).with_context(|| format!("Open body sink for {id} error"))?;
            sink.write_all(contact.to_vcard().as_bytes())
                .with_context(|| format!("Store contact {id} error"))?;
            done(id, contact.change_key.as_deref(), sink)
                .with_context(|| format!("Commit contact {id} error"))?;
        }
        Ok(())
    }

    /// Streams one contact's vCard into `sink`, returning its `changeKey`.
    pub(super) fn get_contact_stream(
        &mut self,
        id: &str,
        mut sink: impl Write,
    ) -> Result<Option<String>> {
        let contact = self.contact(id)?;
        sink.write_all(contact.to_vcard().as_bytes())
            .with_context(|| format!("Stream contact {id} error"))?;
        Ok(contact.change_key)
    }

    /// Creates a contact from a vCard in a folder.
    ///
    /// The contact is read back with its stash: one whose UID did not
    /// survive would come back under an identity minted from its Graph id,
    /// a second person for every other source, so it is deleted again and
    /// the write refused rather than recorded.
    pub(super) fn add_contact(
        &mut self,
        collection: &str,
        mut source: impl Read,
    ) -> Result<WrittenItem> {
        let vcard = read_vcard(&mut source)?;
        let contact = MsgraphContact::create_from_vcard(&vcard)?;
        let uid = contact.stashed_uid();

        let created = self
            .op(|graph| graph.contact_create(folder(collection), &contact))
            .with_context(|| format!("Create contact in {collection} error"))?;
        let stored = self.contact(&created.id)?;

        if uid.is_some() && stored.stashed_uid() != uid {
            if let Err(err) = self.op(|graph| graph.contact_delete(&created.id)) {
                warn!(
                    "cannot delete contact {} that lost its UID: {err}",
                    created.id
                );
            }
            bail!(
                "Graph did not keep the UID of the contact created in {collection}, \
                 so it would read back as another person"
            );
        }

        Ok(WrittenItem {
            id: stored.id,
            revision: stored.change_key,
        })
    }

    /// Replaces a contact from a vCard, sending only what changed against
    /// the server copy, which also serves the `If-Match` check.
    pub(super) fn update_contact(
        &mut self,
        id: &str,
        mut source: impl Read,
        if_match: Option<&str>,
    ) -> Result<Option<String>> {
        let vcard = read_vcard(&mut source)?;
        let current = self.contact(id)?;
        check_revision(id, &current, if_match)?;

        let patch = MsgraphContact::update_from_vcard(&vcard, &current.to_vcard())?;
        let updated = self
            .op(|graph| graph.contact_update(id, &patch))
            .with_context(|| format!("Update contact {id} error"))?;

        Ok(updated.change_key)
    }

    /// Deletes a contact, conditionally on `if_match`.
    pub(super) fn delete_contact(&mut self, id: &str, if_match: Option<&str>) -> Result<()> {
        if if_match.is_some() {
            let current = self.contact(id)?;
            check_revision(id, &current, if_match)?;
        }

        self.op(|graph| graph.contact_delete(id))
            .with_context(|| format!("Delete contact {id} error"))?;
        Ok(())
    }
}

/// The folder segment of a collection id, `None` for the default folder.
fn folder(collection: &str) -> Option<&str> {
    (collection != CONTACTS_FOLDER).then_some(collection)
}

/// Appends a listed contact folder, keyed by id, skipping a nameless one.
fn push_folder(collections: &mut Vec<Collection>, folder: &MsgraphContactFolder) {
    if folder.id.is_empty() {
        return;
    }
    collections.push(Collection {
        id: folder.id.clone(),
        name: folder.display_name.clone(),
        total: None,
        unread: None,
        role: None,
    });
}

/// Reads a whole vCard body as UTF-8 text.
fn read_vcard(source: &mut impl Read) -> Result<String> {
    let mut vcard = String::new();
    source
        .read_to_string(&mut vcard)
        .context("Read the vCard to push to Graph error")?;
    Ok(vcard)
}

/// Refuses a write when the contact moved on Graph since `if_match` was
/// synced, as a DAV server answers 412.
fn check_revision(id: &str, current: &MsgraphContact, if_match: Option<&str>) -> Result<()> {
    match if_match {
        Some(expected) if current.change_key.as_deref() != Some(expected) => bail!(
            "Contact {id} changed on Graph since the last sync, \
             the next run picks the edit up"
        ),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_folder_is_addressed_by_omitting_the_segment() {
        assert_eq!(folder(CONTACTS_FOLDER), None);
        assert_eq!(folder("AAMkADc0"), Some("AAMkADc0"));
    }

    #[test]
    fn a_write_against_a_moved_contact_is_refused() {
        let current = MsgraphContact {
            change_key: Some(String::from("v2")),
            ..Default::default()
        };

        assert!(check_revision("c1", &current, Some("v2")).is_ok());
        assert!(check_revision("c1", &current, None).is_ok());
        assert!(check_revision("c1", &current, Some("v1")).is_err());
    }
}
