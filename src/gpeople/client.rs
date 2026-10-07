//! # Google People client
//!
//! [`GpeopleClient`] wraps io-gpeople's std client behind the adapter
//! surface the sync engine needs, projecting persons onto vCards through
//! io-gpeople's `vcard` feature.
//!
//! The address book is one collection, [`CONTACTS`]: People files every
//! contact under the user's connections, and a contact group is a label a
//! person carries any number of, not a folder.
//!
//! `enumerate` drives the connections listing with a sync token as the
//! engine's opaque checkpoint. A resumed listing reports what changed, a
//! deleted person carrying `metadata.deleted`; an expired token (HTTP 410)
//! restarts a full listing. The person's `etag` is the revision, and People
//! enforces it on update, the etag riding the body.

use std::{
    io::{Read, Write},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use io_gpeople::v1::{
    client::{GpeopleClientStd, GpeopleClientStdConnectOptions, GpeopleClientStdError},
    rest::people::{
        GpeoplePerson, GpeoplePersonField,
        connections::list::GpeopleConnectionsListParams,
        vcard::{GPEOPLE_PERSON_STASH_KEY, GPEOPLE_PERSON_VCARD_FIELDS},
    },
    send::{GPEOPLE_API_BASE, GpeopleSendError, GpeopleSendOutput},
};
use log::{debug, trace, warn};
use pimalaya_stream::{
    proxy::Proxy,
    stream::{Stream, TlsConnectOptions},
    tls::Tls,
};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::{
    client::{EnumEntry, Enumeration, WrittenItem},
    item::{
        collection::Collection,
        flag::{Flag, FlagOp},
    },
    throttle::{Throttle, google_throttled},
};

/// The one collection a People account syncs: every connection.
pub const CONTACTS: &str = "contacts";

/// The person fields of the enumeration: the metadata carries the
/// `deleted` marker of a resumed listing, the rest is read per body.
const ENUMERATION_FIELDS: &[GpeoplePersonField] = &[GpeoplePersonField::Metadata];

/// The page size requested from the connections listing.
const PAGE_SIZE: u32 = 1000;

/// The live People session of one side.
pub struct GpeopleClient {
    inner: GpeopleClientStd,
    /// The TLS configuration, kept for stream reopens.
    tls: Tls,
    /// Whether the server allowed reusing the stream after the last
    /// exchange; when false the next operation reopens it.
    alive: bool,
    /// The source's back-off, shared by its connections.
    throttle: Arc<Throttle>,
}

impl GpeopleClient {
    /// Opens the TLS connection to the People API with a bearer token.
    pub fn connect(token: &SecretString, tls: Tls, throttle: Arc<Throttle>) -> Result<Self> {
        let options = GpeopleClientStdConnectOptions {
            tls: tls.clone(),
            proxy: Proxy::None,
        };
        let inner = GpeopleClientStd::connect(token.expose_secret(), options)
            .context("Cannot connect to Google People")?;

        Ok(Self {
            inner,
            tls,
            alive: true,
            throttle,
        })
    }

    /// Reopens the stream to the People endpoint, keeping the credential.
    fn reconnect(&mut self) -> Result<()> {
        debug!("reopening the people stream");

        let url = Url::parse(GPEOPLE_API_BASE).context("Cannot parse the People API base URL")?;
        let host = url.host_str().context("People API base URL has no host")?;
        let opts = TlsConnectOptions {
            tls: self.tls.clone(),
            ..Default::default()
        };
        let stream = Stream::connect_tls(host, url.port().unwrap_or(443), opts)?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;

        self.inner.set_stream(stream);
        self.alive = true;
        Ok(())
    }

    /// Runs one People operation, reopening the stream first when the
    /// server closed it, and records the new keep-alive hint.
    ///
    /// The source's [`Throttle`] sends it again while Google throttles it
    /// (429, 503, or a 403 rate limit).
    fn op<T>(
        &mut self,
        mut run: impl FnMut(
            &mut GpeopleClientStd,
        ) -> Result<GpeopleSendOutput<T>, GpeopleClientStdError>,
    ) -> Result<T, GpeopleClientStdError> {
        let throttle = Arc::clone(&self.throttle);
        throttle.call(
            1,
            || {
                if !self.alive
                    && let Err(err) = self.reconnect()
                {
                    warn!("cannot reopen the people stream: {err:#}");
                }

                let out = run(&mut self.inner)?;
                self.alive = out.keep_alive;
                Ok(out.response)
            },
            |err| match err {
                GpeopleClientStdError::Send(GpeopleSendError::Api { status, message }) => {
                    google_throttled(*status, message)
                }
                _ => None,
            },
            |message| {
                GpeopleClientStdError::Send(GpeopleSendError::Api {
                    status: 429,
                    message,
                })
            },
        )
    }

    /// Lists the one address book a People account has.
    pub fn list_collections(&mut self) -> Result<Vec<Collection>> {
        Ok(vec![Collection {
            id: CONTACTS.to_owned(),
            name: String::from("Contacts"),
            total: None,
            unread: None,
            role: Some(String::from("default")),
        }])
    }

    /// The one address book, the default by construction. A check-only
    /// probe.
    pub fn default_collection(&self) -> Option<String> {
        Some(CONTACTS.to_owned())
    }

    /// Enumerates the connections, incrementally from the checkpoint's sync
    /// token when it holds one and the token has not expired.
    pub fn enumerate(&mut self, collection: &str, cursor: Option<&[u8]>) -> Result<Enumeration> {
        ensure_contacts(collection)?;

        let token = cursor.and_then(|bytes| String::from_utf8(bytes.to_vec()).ok());
        let (persons, complete, next) = match token {
            None => self.list(None)?,
            Some(token) => match self.list(Some(&token)) {
                Err(err) if is_expired(&err) => {
                    warn!("people sync token expired, restarting a full listing");
                    self.list(None)?
                }
                result => result?,
            },
        };

        let mut items = Vec::new();
        let mut vanished = Vec::new();
        for person in persons {
            let id = person.id().to_owned();
            if id.is_empty() {
                continue;
            }
            let deleted = person
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.deleted)
                .unwrap_or(false);
            if deleted {
                if !complete {
                    vanished.push(id);
                }
            } else {
                items.push(EnumEntry {
                    id,
                    flags: Default::default(),
                    revision: Some(person.etag).filter(|etag| !etag.is_empty()),
                });
            }
        }

        Ok(Enumeration {
            items,
            vanished,
            complete,
            checkpoint: next.into_bytes(),
        })
    }

    /// Pages through the connections listing, returning the persons,
    /// whether the listing was full, and the next sync token.
    fn list(&mut self, token: Option<&str>) -> Result<(Vec<GpeoplePerson>, bool, String)> {
        debug!("begin people connections listing");
        trace!("resumed: {}", token.is_some());

        let mut persons = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let params = GpeopleConnectionsListParams {
                page_size: Some(PAGE_SIZE),
                page_token: page_token.as_deref(),
                request_sync_token: true,
                sync_token: token,
                ..Default::default()
            };
            let page = self
                .op(|people| people.connections_list(ENUMERATION_FIELDS, &params))
                .map_err(anyhow::Error::new)
                .context("List People connections error")?;

            persons.extend(page.connections);

            match page.next_page_token {
                Some(next) => page_token = Some(next),
                None => {
                    let next = page
                        .next_sync_token
                        .context("The People listing returned no sync token")?;
                    debug!("end of people connections listing");
                    trace!("persons: {}", persons.len());
                    return Ok((persons, token.is_none(), next));
                }
            }
        }
    }

    /// Reads one person with every field the projection manages.
    fn person(&mut self, id: &str) -> Result<GpeoplePerson> {
        let resource_name = format!("people/{id}");
        self.op(|people| people.person_get(&resource_name, GPEOPLE_PERSON_VCARD_FIELDS, &[]))
            .with_context(|| format!("Get person {id} error"))
    }

    /// Streams the vCards of an id set, each committed with its etag.
    pub fn fetch_bodies<S: Write>(
        &mut self,
        collection: &str,
        ids: &[&str],
        mut open: impl FnMut(&str) -> std::io::Result<S>,
        mut done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<()> {
        ensure_contacts(collection)?;

        for id in ids {
            let person = self.person(id)?;
            let mut sink = open(id).with_context(|| format!("Open body sink for {id} error"))?;
            sink.write_all(person.to_vcard().as_bytes())
                .with_context(|| format!("Store person {id} error"))?;
            done(id, Some(person.etag.as_str()), sink)
                .with_context(|| format!("Commit person {id} error"))?;
        }
        Ok(())
    }

    /// Streams one person's vCard into `sink`, returning its etag.
    pub fn get_item_stream(
        &mut self,
        collection: &str,
        id: &str,
        mut sink: impl Write,
    ) -> Result<Option<String>> {
        ensure_contacts(collection)?;

        let person = self.person(id)?;
        sink.write_all(person.to_vcard().as_bytes())
            .with_context(|| format!("Stream person {id} error"))?;
        Ok(Some(person.etag))
    }

    /// Creates a person from a vCard.
    ///
    /// A person whose UID did not survive the write would read back under
    /// one minted from its resource name, a second person for every other
    /// source, so it is deleted again and the write refused.
    pub fn add_item_stream(&mut self, collection: &str, source: impl Read) -> Result<WrittenItem> {
        ensure_contacts(collection)?;

        let vcard = read_vcard(source)?;
        let person = GpeoplePerson::from_vcard(&vcard)?;
        let uid = person.stashed_uid();

        let created = self
            .op(|people| people.contact_create(&person, GPEOPLE_PERSON_VCARD_FIELDS, &[]))
            .context("Create People contact error")?;

        if uid.is_some() && created.stashed_uid() != uid {
            let resource_name = created.resource_name.clone();
            if let Err(err) = self.op(|people| people.contact_delete(&resource_name)) {
                warn!("cannot delete person {resource_name} that lost its UID: {err}");
            }
            bail!(
                "People did not keep the UID of the created contact, so it would read back as another person"
            );
        }

        Ok(WrittenItem {
            id: created.id().to_owned(),
            revision: Some(created.etag),
        })
    }

    /// Replaces a person from a vCard, the update mask shrunk to what
    /// differs from the server copy and the etag guarding the write.
    ///
    /// A masked update replaces the whole `clientData` list, so the
    /// entries other clients own there are merged back first.
    pub fn update_item_stream(
        &mut self,
        id: &str,
        source: impl Read,
        if_match: Option<&str>,
    ) -> Result<Option<String>> {
        let vcard = read_vcard(source)?;
        let current = self.person(id)?;

        let mut person = GpeoplePerson::from_vcard(&vcard)?;
        person.resource_name = current.resource_name.clone();

        let base = GpeoplePerson::from_vcard(&current.to_vcard())?;
        let lost = person.unremovable_properties(&base);
        if !lost.is_empty() {
            warn!("people keeps stashed properties {lost:?} of {id} the edit dropped");
        }

        let fields = person.changed_fields(&base);
        if fields.is_empty() {
            return Ok(Some(current.etag));
        }

        if fields.contains(&GpeoplePersonField::ClientData) {
            let mut merged: Vec<_> = current
                .client_data
                .iter()
                .filter(|entry| entry.key.as_deref() != Some(GPEOPLE_PERSON_STASH_KEY))
                .cloned()
                .collect();
            merged.append(&mut person.client_data);
            person.client_data = merged;
        }

        person.etag = if_match.map_or(current.etag, str::to_owned);

        let updated = self
            .op(|people| people.contact_update(&person, &fields, GPEOPLE_PERSON_VCARD_FIELDS, &[]))
            .with_context(|| format!("Update person {id} error"))?;

        Ok(Some(updated.etag))
    }

    /// Deletes a person, refused when it moved since `if_match`, People
    /// taking no etag on delete.
    pub fn delete_item(&mut self, id: &str, if_match: Option<&str>) -> Result<()> {
        if let Some(expected) = if_match {
            let current = self.person(id)?;
            if current.etag != expected {
                bail!(
                    "Person {id} changed on Google since the last sync, the next run picks the edit up"
                );
            }
        }

        let resource_name = format!("people/{id}");
        self.op(|people| people.contact_delete(&resource_name))
            .with_context(|| format!("Delete person {id} error"))?;
        Ok(())
    }

    /// Rejected: People contacts have no flags.
    pub fn store_flags(&mut self, _ids: &[&str], _flags: &[Flag], _op: FlagOp) -> Result<()> {
        bail!("Google contacts have no flags (store not supported)")
    }
}

/// Refuses any collection but the one People has.
fn ensure_contacts(collection: &str) -> Result<()> {
    if collection != CONTACTS {
        bail!("Google People has one address book, `{CONTACTS}`, not `{collection}`");
    }
    Ok(())
}

/// Whether a listing failed on an expired sync token: HTTP 410, or the 400
/// People answers as often, naming the expired token
/// (`FAILED_PRECONDITION`, `EXPIRED_SYNC_TOKEN`).
fn is_expired(err: &anyhow::Error) -> bool {
    err.chain().any(
        |cause| match cause.downcast_ref::<GpeopleClientStdError>() {
            Some(GpeopleClientStdError::Send(send)) => {
                send.status() == Some(410)
                    || (send.status() == Some(400) && names_expired_token(&cause.to_string()))
            }
            _ => false,
        },
    )
}

/// Whether an error text names an expired sync token.
fn names_expired_token(text: &str) -> bool {
    text.contains("EXPIRED_SYNC_TOKEN") || text.contains("Sync token is expired")
}

/// Reads a whole vCard body as UTF-8 text.
fn read_vcard(mut source: impl Read) -> Result<String> {
    let mut vcard = String::new();
    source
        .read_to_string(&mut vcard)
        .context("Read the vCard to push to Google error")?;
    Ok(vcard)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// People answers an expired sync token with a 400 as often as a 410
    /// (seen on CI, 2026-10-05), which restarts a full listing too.
    #[test]
    fn an_expired_sync_token_is_named_in_a_400() {
        assert!(names_expired_token(
            "People API returned HTTP 400: Sync token is expired. Clear local cache and retry call without the sync token."
        ));
        assert!(names_expired_token(
            "400 FAILED_PRECONDITION EXPIRED_SYNC_TOKEN"
        ));
        assert!(!names_expired_token(
            "People API returned HTTP 400: Invalid personFields mask"
        ));
    }

    #[test]
    fn people_has_one_address_book() {
        assert!(ensure_contacts(CONTACTS).is_ok());
        assert!(ensure_contacts("friends").is_err());
    }
}
