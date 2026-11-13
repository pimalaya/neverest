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
        vcard::{GPEOPLE_PERSON_VCARD_FIELDS, GpeoplePersonVcard},
    },
    send::{GPEOPLE_API_BASE, GpeopleApiError, GpeopleSendError, GpeopleSendOutput},
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
    client::{EnumEntry, Enumeration, Gone, WrittenItem},
    item::{collection::Collection, flag::Flag},
    throttle::{GoogleAnswer, GoogleWaits, Request, Throttle},
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
    /// (429, a 502 to 504, or a 403 rate limit). A request creating a contact
    /// goes through [`create`](Self::create) instead.
    fn op<T>(
        &mut self,
        run: impl FnMut(&mut GpeopleClientStd) -> Result<GpeopleSendOutput<T>, GpeopleClientStdError>,
    ) -> Result<T, GpeopleClientStdError> {
        self.send(Request::Idempotent, run)
    }

    /// Runs one People request creating a contact: sent again on a refusal
    /// Google answers before doing anything (429, a rate limit), never on a
    /// 503, after which the contact may exist.
    fn create<T>(
        &mut self,
        run: impl FnMut(&mut GpeopleClientStd) -> Result<GpeopleSendOutput<T>, GpeopleClientStdError>,
    ) -> Result<T, GpeopleClientStdError> {
        self.send(Request::Create, run)
    }

    /// Runs one People request of `request`, as [`op`](Self::op) describes.
    fn send<T>(
        &mut self,
        request: Request,
        mut run: impl FnMut(
            &mut GpeopleClientStd,
        ) -> Result<GpeopleSendOutput<T>, GpeopleClientStdError>,
    ) -> Result<T, GpeopleClientStdError> {
        let throttle = Arc::clone(&self.throttle);
        let waits = GoogleWaits::new();
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
                GpeopleClientStdError::Send(GpeopleSendError::Api(err)) => waits.throttled(
                    request,
                    &GoogleAnswer {
                        status: err.status,
                        message: &err.message,
                        reasons: &err.reasons,
                        detail_reasons: &err.detail_reasons,
                        google_status: err.google_status.as_deref(),
                    },
                ),
                _ => None,
            },
            |message| {
                GpeopleClientStdError::Send(GpeopleSendError::Api(GpeopleApiError {
                    status: 429,
                    message,
                    ..Default::default()
                }))
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
            takes_tasks: None,
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
                items.push(EnumEntry::bare(
                    id,
                    Default::default(),
                    Some(person.etag).filter(|etag| !etag.is_empty()),
                ));
            }
        }

        Ok(Enumeration {
            items,
            vanished,
            complete,
            checkpoint: Some(next.into_bytes()),
            ..Default::default()
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
    ///
    /// A person People answers missing (404, 410) fails with [`Gone`]: one
    /// deleted since its listing is owed nothing more in the run, rather
    /// than failing the address book's fetch in every run.
    pub fn get_item_stream(
        &mut self,
        collection: &str,
        id: &str,
        mut sink: impl Write,
    ) -> Result<Option<String>> {
        ensure_contacts(collection)?;

        let person = match self.person(id) {
            Ok(person) => person,
            Err(err) if is_missing(&err) => {
                return Err(err.context(Gone(format!("{id} in {collection}"))));
            }
            Err(err) => return Err(err),
        };
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
        let GpeoplePersonVcard { person, dropped } = GpeoplePerson::from_vcard(&vcard)?;
        if !dropped.is_empty() {
            warn!("people cannot hold the properties {dropped:?} of a new contact, too long");
        }
        let uid = person.stashed_uid();

        let created = self
            .create(|people| people.contact_create(&person, GPEOPLE_PERSON_VCARD_FIELDS, &[]))
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
    /// A masked update replaces each masked field whole, so what People
    /// holds there and a vCard cannot show is merged back first.
    pub fn update_item_stream(
        &mut self,
        id: &str,
        source: impl Read,
        if_match: Option<&str>,
    ) -> Result<Option<String>> {
        let vcard = read_vcard(source)?;
        let current = self.person(id)?;

        let GpeoplePersonVcard {
            mut person,
            dropped,
        } = GpeoplePerson::from_vcard(&vcard)?;
        if !dropped.is_empty() {
            warn!("people cannot hold the properties {dropped:?} of {id}, too long");
        }
        person.resource_name = current.resource_name.clone();

        let base = GpeoplePerson::from_vcard(&current.to_vcard())?.person;
        let lost = person.unremovable_properties(&base);
        if !lost.is_empty() {
            warn!("people keeps stashed properties {lost:?} of {id} the edit dropped");
        }

        let fields = person.changed_fields(&base);
        if fields.is_empty() {
            return Ok(Some(current.etag));
        }

        person.merge_unprojected(&current, &fields);

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
    pub fn update_flags(&mut self, _ids: &[&str], _add: &[Flag], _remove: &[Flag]) -> Result<()> {
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

/// Whether a read failed on a person People no longer holds (HTTP 404, 410).
fn is_missing(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<GpeopleClientStdError>(),
            Some(GpeopleClientStdError::Send(send)) if matches!(send.status(), Some(404 | 410))
        )
    })
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
    use std::{collections::VecDeque, sync::Mutex};

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

    /// A stream answering one canned response, discarding what is written.
    struct Canned(Vec<u8>);

    impl Read for Canned {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = self.0.len().min(buf.len());
            buf[..n].copy_from_slice(&self.0[..n]);
            self.0.drain(..n);
            Ok(n)
        }
    }

    impl Write for Canned {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// A person deleted between its listing and its read is gone: failing
    /// instead, it would fail the fetch of the address book in every run
    /// until a listing took it away.
    #[test]
    fn a_person_missing_from_its_read_is_gone() {
        for status in ["404 Not Found", "410 Gone"] {
            let body = r#"{"error":{"code":404,"message":"Not Found"}}"#;
            let answer = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let mut client = GpeopleClient {
                inner: GpeopleClientStd::new(Canned(answer.into_bytes()), "token"),
                tls: Tls::default(),
                alive: true,
                throttle: Arc::new(Throttle::new("gpeople")),
            };

            let err = client
                .get_item_stream(CONTACTS, "c1", Vec::new())
                .expect_err("no person to read");

            assert!(Gone::is(&err), "{status}: {err:#}");
        }
    }

    /// A stream answering each request with the next scripted answer, and
    /// keeping each request written to it.
    struct Scripted {
        answers: VecDeque<Vec<u8>>,
        out: Vec<u8>,
        writing: bool,
        requests: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl Read for Scripted {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.writing {
                self.writing = false;
                self.out = self.answers.pop_front().unwrap_or_default();
            }
            let n = self.out.len().min(buf.len());
            buf[..n].copy_from_slice(&self.out[..n]);
            self.out.drain(..n);
            Ok(n)
        }
    }

    impl Write for Scripted {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut requests = self.requests.lock().unwrap();
            if !self.writing {
                self.writing = true;
                requests.push(Vec::new());
            }
            requests.last_mut().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// An update replaces each masked field whole: built from the vCard
    /// alone, it would erase the second organization and the data of
    /// another client, neither of which a vCard shows.
    #[test]
    fn an_update_keeps_what_the_vcard_cannot_show() {
        let held = r#"{"resourceName":"people/c1","etag":"e1",
            "names":[{"givenName":"John","familyName":"Doe","unstructuredName":"John Doe"}],
            "organizations":[{"name":"Acme","title":"Engineer"},{"name":"Initech","title":"Advisor"}],
            "clientData":[{"key":"other-client","value":"kept"}]}"#;
        let answer = |body: &str| {
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .into_bytes()
        };
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stream = Scripted {
            answers: VecDeque::from([answer(held), answer(&held.replace("e1", "e2"))]),
            out: Vec::new(),
            writing: false,
            requests: Arc::clone(&requests),
        };
        let mut client = GpeopleClient {
            inner: GpeopleClientStd::new(stream, "token"),
            tls: Tls::default(),
            alive: true,
            throttle: Arc::new(Throttle::new("gpeople")),
        };

        let current: GpeoplePerson = serde_json::from_str(held).unwrap();
        let vcard = current.to_vcard();
        assert!(
            vcard.contains("Engineer") && !vcard.contains("Initech"),
            "{vcard}"
        );
        // NOTE: a line People has no field for is stashed in `clientData`.
        let edited = vcard
            .replace("Engineer", "Director")
            .replace("END:VCARD", "X-PET:Rex\r\nEND:VCARD");

        let etag = client
            .update_item_stream("c1", edited.as_bytes(), Some("e1"))
            .unwrap();
        assert_eq!(etag.as_deref(), Some("e2"));

        let requests = requests.lock().unwrap();
        let patch = String::from_utf8_lossy(&requests[1]);
        let (head, body) = patch.split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("PATCH "), "{head}");
        let sent: GpeoplePerson = serde_json::from_str(body).unwrap();

        let organizations: Vec<_> = sent
            .organizations
            .iter()
            .map(|org| (org.name.as_deref(), org.title.as_deref()))
            .collect();
        assert_eq!(
            organizations,
            [
                (Some("Acme"), Some("Director")),
                (Some("Initech"), Some("Advisor"))
            ]
        );
        assert!(
            sent.client_data
                .iter()
                .any(|entry| entry.key.as_deref() == Some("other-client")),
            "{:?}",
            sent.client_data
        );
    }
}
