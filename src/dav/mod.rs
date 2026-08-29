//! # DAV backends
//!
//! [`client`] wraps the io-webdav session behind the shared cross-protocol
//! client surface: collections, RFC 6578 `sync-collection` enumeration,
//! multiget bodies and conditional writes.
//!
//! One adapter serves both CardDAV (RFC 6352) and CalDAV (RFC 4791), which
//! differ only in the home set they discover, the collection they list and the
//! extension a new resource is named with ([`client::DavKind`]). These are the
//! mutable-content backends, the ones exercising the revision machinery.

pub mod client;
