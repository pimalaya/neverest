//! DAV backends: the protocol-direct client adapter.
//!
//! [`client`] wraps the io-webdav session behind the shared cross-protocol
//! client surface (address books and calendars as collections, RFC 6578
//! `sync-collection` enumeration, multiget bodies, and conditional writes).
//!
//! One adapter serves both CardDAV (RFC 6352) and CalDAV (RFC 4791): they
//! differ in the home set they discover, the collection they list and the
//! extension a new resource is named with, and in nothing else the sync sees.
//! Which of the two a session speaks is its [`client::DavKind`].
//!
//! These are the **mutable-content** backends: a card or an event is edited in
//! place under an ETag rather than replaced, so they are the ones that
//! exercise the revision and conflict machinery the mail backends leave inert.

pub mod client;
