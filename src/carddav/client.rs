//! CardDAV client adapter for the shared cross-protocol client.
//!
//! [`CarddavClient`] wraps the std blocking io-webdav client behind the same
//! adapter surface as the IMAP and Graph backends. It is the first
//! **mutable-content** backend: a card is edited in place rather than
//! replaced, so this is where the revision plumbing (ETags, `If-Match`) built
//! blind against mail finally has a server that exercises it.
//!
//! Three shapes differ from the mail backends:
//!
//! - **Collections are address books**, keyed by their path segment id rather
//!   than their display name. A display name is optional, may collide between
//!   two address books and may change under a running sync, none of which a
//!   collection key survives.
//! - **Items resolve at `Full` only.** A `sync-collection` REPORT returns
//!   hrefs and ETags, never a `UID`, so there is no cheap `Meta` tier and
//!   [`crate::kind::Kind::parse_summary`] is `None` for cards.
//! - **Flags are known-empty, not unknown.** CardDAV has no flag concept, so
//!   every entry reports an empty set, which the engine reads as "no flags"
//!   rather than "not fetched".
//!
//! Enumeration is RFC 6578 throughout: an initial report (no token) returns
//! the whole member set plus the token, and a later one returns only what
//! changed. A server that rejects the stored token
//! ([`WebdavSyncCollectionError::InvalidSyncToken`]) is answered with a fresh full
//! report, which the engine reads as a complete snapshot and reconciles
//! against, exactly as an expired Graph delta link or an IMAP UIDVALIDITY
//! bump would be. A truncated report (RFC 6578 §3.6) is drained by running it
//! again from the token it returned.

use std::{
    collections::BTreeSet,
    io::{ErrorKind, Read, Write},
};

use anyhow::{Context, Result, bail};
use io_http::rfc9112::send::Http11SendError;
use io_webdav::{
    client::{WebdavClientStd, WebdavClientStdError},
    rfc4918::{WebdavAuth, follow_redirects::WebdavFollowRedirectsError, send::WebdavSendError},
    rfc6352::addressbook::CarddavAddressbook,
    rfc6578::sync_collection::WebdavSyncCollectionError,
};
use log::{debug, warn};
use pimalaya_stream::tls::Tls;
use url::Url;

use crate::{
    client::{EnumEntry, Enumeration, WrittenItem},
    item::{collection::Collection, flag::Flag, flag::FlagOp},
};

/// How many truncated rounds a single enumeration drains before giving up,
/// so a server answering "truncated" forever cannot spin the sync.
const MAX_SYNC_ROUNDS: usize = 32;

/// A live CardDAV session, scoped to one address book home set.
pub struct CarddavClient {
    inner: WebdavClientStd,
    /// The connect arguments, kept so a connection the server closed can be
    /// reopened (see [`op`](CarddavClient::op)).
    server: Url,
    tls: Tls,
    auth: WebdavAuth,
}

impl CarddavClient {
    /// Opens the session and discovers the address book home set, so a
    /// misconfigured URL or credential fails here rather than at the first
    /// enumeration.
    pub fn connect(server: &Url, tls: &Tls, auth: WebdavAuth) -> Result<Self> {
        let inner = WebdavClientStd::connect(server, tls, auth.clone())
            .context("Cannot connect to the CardDAV server")?;
        let mut client = Self {
            inner,
            server: server.clone(),
            tls: tls.clone(),
            auth,
        };

        let home = client
            .op(WebdavClientStd::addressbook_home_set)
            .context("Cannot discover the CardDAV address book home set")?;
        debug!("carddav address book home set: {home}");

        Ok(client)
    }

    /// Runs one WebDAV exchange, reopening the connection and running it
    /// again when the server had closed it.
    ///
    /// io-webdav holds a single stream and reports no keep-alive hint, so a
    /// server that answers HTTP/1.0 (Radicale's built-in server) or sends
    /// `Connection: close` leaves the next request written into a socket the
    /// peer already hung up on, and everything after the first exchange
    /// fails. The Graph backend reconnects on the hint its client does
    /// report; this is the same repair without one.
    ///
    /// Only an end-of-stream failure is retried, which is the shape of a
    /// request the server never read, so a create or a delete is not
    /// replayed against a server that acted on it. The reopened client keeps
    /// the discovered principal and home-set URLs, so a reconnect costs a
    /// handshake and no discovery.
    fn op<T>(
        &mut self,
        mut run: impl FnMut(&mut WebdavClientStd) -> Result<T, WebdavClientStdError>,
    ) -> Result<T, WebdavClientStdError> {
        match run(&mut self.inner) {
            Err(err) if is_connection_closed(&err) => {
                debug!("carddav connection closed by the server, reopening");
                self.reconnect()?;
                run(&mut self.inner)
            }
            out => out,
        }
    }

    /// Reopens the connection, carrying the discovery this session already
    /// paid for over to the new client.
    fn reconnect(&mut self) -> Result<(), WebdavClientStdError> {
        let mut inner = WebdavClientStd::connect(&self.server, &self.tls, self.auth.clone())?;
        inner.principal_url = self.inner.principal_url.clone();
        inner.addressbook_home_set = self.inner.addressbook_home_set.clone();
        self.inner = inner;

        Ok(())
    }

    /// Lists every address book. Counts are never reported: CardDAV has no
    /// cheap total, and paying a full enumeration per address book to render
    /// one number is not a trade this makes.
    pub fn list_collections(&mut self, _with_counts: bool) -> Result<Vec<Collection>> {
        let books = self
            .op(WebdavClientStd::list_addressbooks)
            .context("Cannot list the CardDAV address books")?;

        Ok(books.into_iter().map(collection).collect())
    }

    /// Creates an address book, named after the collection key.
    pub fn create_collection(&mut self, collection: &str) -> Result<()> {
        let book = CarddavAddressbook {
            id: collection.to_owned(),
            display_name: Some(collection.to_owned()),
            ..Default::default()
        };

        self.op(|dav| dav.create_addressbook(&book))
            .with_context(|| format!("Cannot create the address book `{collection}`"))
    }

    /// Deletes an address book.
    pub fn delete_collection(&mut self, collection: &str) -> Result<()> {
        self.op(|dav| dav.delete_addressbook(collection))
            .with_context(|| format!("Cannot delete the address book `{collection}`"))
    }

    /// Enumerates an address book through `sync-collection`, carrying the
    /// server's sync token as the engine's opaque checkpoint.
    pub fn enumerate(&mut self, collection: &str, cursor: Option<&[u8]>) -> Result<Enumeration> {
        let token = cursor.map(String::from_utf8_lossy).map(String::from);

        match self.sync(collection, token.as_deref()) {
            Ok(enumeration) => Ok(enumeration),
            // The server forgot the token (RFC 6578 §3.2 `valid-sync-token`),
            // so the delta is unrecoverable and a full snapshot takes over.
            // Handles are hrefs and survive it, so no rebuild follows.
            Err(err) if is_invalid_sync_token(&err) => {
                warn!("carddav sync token rejected for `{collection}`, enumerating in full");
                self.sync(collection, None)
                    .with_context(|| format!("Cannot enumerate `{collection}` in full"))
            }
            Err(err) => {
                Err(anyhow::Error::new(err).context(format!("Cannot enumerate `{collection}`")))
            }
        }
    }

    /// One enumeration, draining every truncated round into a single result.
    fn sync(
        &mut self,
        collection: &str,
        token: Option<&str>,
    ) -> Result<Enumeration, WebdavClientStdError> {
        let complete = token.is_none();
        let mut items = Vec::new();
        let mut vanished = Vec::new();
        let mut token = token.map(String::from);

        for round in 0..MAX_SYNC_ROUNDS {
            let delta = self.op(|dav| dav.sync_cards(collection, token.as_deref()))?;

            items.extend(delta.changed.into_iter().map(|change| EnumEntry {
                id: href_id(&change.href),
                flags: BTreeSet::new(),
                revision: change.etag,
            }));
            vanished.extend(delta.vanished.iter().map(|href| href_id(href)));
            token = delta.sync_token;

            if !delta.truncated {
                break;
            }
            if round + 1 == MAX_SYNC_ROUNDS {
                warn!(
                    "carddav server kept truncating `{collection}` after {MAX_SYNC_ROUNDS} rounds, \
                     continuing with what it returned"
                );
            }
        }

        Ok(Enumeration {
            items,
            vanished,
            complete,
            checkpoint: token.unwrap_or_default().into_bytes(),
        })
    }

    /// Batch-fetches card bodies in one `addressbook-multiget` round-trip,
    /// routing each into the sink the caller opens for it.
    pub fn fetch_bodies<S: Write>(
        &mut self,
        collection: &str,
        ids: &[&str],
        mut open: impl FnMut(&str) -> std::io::Result<S>,
        mut done: impl FnMut(&str, Option<&str>, S) -> std::io::Result<()>,
    ) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let cards = self
            .op(|dav| dav.multiget_cards(collection, ids))
            .with_context(|| format!("Cannot fetch cards from `{collection}`"))?;

        for card in cards {
            let mut sink = open(&card.id)?;
            sink.write_all(&card.data)?;
            // The ETag the body was fetched at rides with it, so the engine
            // records the revision it actually stored.
            done(&card.id, card.etag.as_deref(), sink)?;
        }

        Ok(())
    }

    /// Streams one card's raw bytes, returning the ETag they correspond to.
    pub fn get_item_stream(
        &mut self,
        collection: &str,
        id: &str,
        mut sink: impl Write,
    ) -> Result<Option<String>> {
        let card = self
            .op(|dav| dav.read_card(collection, id))
            .with_context(|| format!("Cannot read the card `{id}`"))?;

        sink.write_all(&card.data)?;

        Ok(card.etag)
    }

    /// Creates a card, addressing it by `link_hint` (its `UID`) so the href a
    /// server assigns stays derivable from the card itself.
    pub fn add_item_stream(
        &mut self,
        collection: &str,
        mut source: impl Read,
        link_hint: Option<&str>,
    ) -> Result<WrittenItem> {
        let mut vcard = Vec::new();
        source.read_to_end(&mut vcard)?;

        let id = card_id(link_hint, &vcard);
        let created = self
            .op(|dav| dav.create_card(collection, &id, vcard.clone()))
            .with_context(|| format!("Cannot create the card `{id}` in `{collection}`"))?;

        Ok(WrittenItem {
            id: created.id,
            revision: created.etag,
        })
    }

    /// Replaces a card in place, conditionally on the last-synced ETag: a
    /// server whose copy moved since rejects the write rather than losing the
    /// other edit, which is what the engine's conflict path is waiting for.
    pub fn update_item_stream(
        &mut self,
        collection: &str,
        id: &str,
        mut source: impl Read,
        if_match: Option<&str>,
    ) -> Result<Option<String>> {
        let mut vcard = Vec::new();
        source.read_to_end(&mut vcard)?;

        let updated = self
            .op(|dav| dav.update_card(collection, id, vcard.clone(), if_match))
            .with_context(|| format!("Cannot update the card `{id}` in `{collection}`"))?;

        Ok(updated.etag)
    }

    /// Deletes a card, conditionally on the last-synced ETag.
    pub fn delete_item(
        &mut self,
        collection: &str,
        id: &str,
        if_match: Option<&str>,
    ) -> Result<()> {
        self.op(|dav| dav.delete_card(collection, id, if_match))
            .with_context(|| format!("Cannot delete the card `{id}` from `{collection}`"))
    }

    /// Moves cards between address books. CardDAV has no server-side move, so
    /// each card is re-created at the target and deleted from the source; the
    /// delete only runs once the create was accepted, so a failure leaves the
    /// card where it was rather than nowhere.
    pub fn move_items(&mut self, from: &str, to: &str, ids: &[&str]) -> Result<()> {
        for id in ids {
            let card = self
                .op(|dav| dav.read_card(from, id))
                .with_context(|| format!("Cannot read the card `{id}` for a move"))?;

            self.op(|dav| dav.create_card(to, id, card.data.clone()))
                .with_context(|| format!("Cannot create the card `{id}` in `{to}`"))?;

            self.op(|dav| dav.delete_card(from, id, card.etag.as_deref()))
                .with_context(|| format!("Cannot delete the moved card `{id}` from `{from}`"))?;
        }

        Ok(())
    }

    /// Rejected: CardDAV has no flags, and the enumeration reports every card
    /// as known-empty, so the engine never derives a flag change to push. A
    /// call here means something upstream invented one.
    pub fn store_flags(&mut self, _ids: &[&str], _flags: &[Flag], _op: FlagOp) -> Result<()> {
        bail!("CardDAV has no flags (store not supported)")
    }
}

/// One address book as a collection. Both the id and the name are the path
/// segment: a collection key must be stable and unique, and a DAV display
/// name is neither (optional, mutable, free to collide).
fn collection(book: CarddavAddressbook) -> Collection {
    Collection {
        id: book.id.clone(),
        name: book.id,
        total: None,
        unread: None,
    }
}

/// The addressing id of a member href: its last non-empty path segment,
/// exactly as io-webdav addresses cards.
fn href_id(href: &str) -> String {
    href.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(href)
        .to_owned()
}

/// The resource name a new card is created under: its `UID` plus the
/// conventional extension, falling back to the body digest when the card
/// carries no `UID` (the same fallback its link id took).
fn card_id(link_hint: Option<&str>, vcard: &[u8]) -> String {
    match link_hint {
        Some(uid) if !uid.trim().is_empty() => format!("{}.vcf", sanitize(uid.trim())),
        _ => {
            let (link, _, _) = crate::kind::vcard::parse_body(vcard, vcard.len() as u64);
            format!("{}.vcf", sanitize(link.0.trim_start_matches("hash:")))
        }
    }
}

/// Keeps a `UID` addressable as one path segment: a `UID` is free-form text
/// and may legally carry a `/` or a space, neither of which survives being
/// spliced into a URL path.
fn sanitize(uid: &str) -> String {
    uid.chars()
        .map(|char| match char {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | ':' => char,
            _ => '-',
        })
        .collect()
}

/// Whether the exchange died on a connection the server had already closed,
/// the one failure [`CarddavClient::op`] repairs by reopening it.
///
/// An end of stream where a response was due says the request was never
/// answered, and a broken pipe or a reset says it was never written, so
/// neither leaves a half-applied write behind. Every other failure is the
/// server's answer and is reported as it is.
fn is_connection_closed(err: &WebdavClientStdError) -> bool {
    match err {
        WebdavClientStdError::Send(WebdavSendError::Send(Http11SendError::Eof))
        | WebdavClientStdError::WebdavFollowRedirects(WebdavFollowRedirectsError::Send(
            Http11SendError::Eof,
        ))
        | WebdavClientStdError::WebdavSyncCollection(WebdavSyncCollectionError::Send(
            WebdavSendError::Send(Http11SendError::Eof),
        )) => true,
        WebdavClientStdError::Io(err) => matches!(
            err.kind(),
            ErrorKind::BrokenPipe | ErrorKind::ConnectionAborted | ErrorKind::ConnectionReset
        ),
        _ => false,
    }
}

/// Whether the server rejected the sync token, the one enumeration failure
/// that is recoverable by falling back to a full report.
fn is_invalid_sync_token(err: &WebdavClientStdError) -> bool {
    matches!(
        err,
        WebdavClientStdError::WebdavSyncCollection(WebdavSyncCollectionError::InvalidSyncToken)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_href_addresses_by_its_last_segment() {
        assert_eq!(href_id("/dav/books/default/card-1.vcf"), "card-1.vcf");
        assert_eq!(
            href_id("https://dav.example.org/books/default/card-1.vcf"),
            "card-1.vcf"
        );
        // A collection href may carry a trailing slash.
        assert_eq!(href_id("/dav/books/default/"), "default");
        assert_eq!(href_id("card-1.vcf"), "card-1.vcf");
    }

    #[test]
    fn a_new_card_is_addressed_by_its_uid() {
        assert_eq!(card_id(Some("card-1"), b""), "card-1.vcf");

        // A UID is free-form text: anything that would not survive a URL path
        // is folded away rather than escaped, since the id only has to be
        // stable and unique, not reversible.
        assert_eq!(
            card_id(Some("urn:uuid:4fbe8971-0bc3"), b""),
            "urn:uuid:4fbe8971-0bc3.vcf"
        );
        assert_eq!(card_id(Some("a b/c"), b""), "a-b-c.vcf");
    }

    #[test]
    fn a_card_without_a_uid_is_addressed_by_its_body_digest() {
        let raw = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:No Uid\r\nEND:VCARD\r\n";

        let id = card_id(None, raw);
        assert!(id.ends_with(".vcf"), "got {id}");
        assert!(!id.starts_with("hash:"), "the prefix is not a path segment");
        // Deterministic: the same body must address the same resource.
        assert_eq!(id, card_id(None, raw));
    }
}
