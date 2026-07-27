//! CardDAV backend: the protocol-direct client adapter.
//!
//! [`client`] wraps the io-webdav session behind the shared cross-protocol
//! client surface (address books as collections, RFC 6578 `sync-collection`
//! enumeration, `addressbook-multiget` bodies, and conditional writes).
//!
//! It is the first backend whose items are **mutable**: a card is edited in
//! place under an ETag rather than replaced, so it is the one that exercises
//! the revision and conflict machinery the mail backends leave inert.

pub mod client;
