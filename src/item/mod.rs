//! The kind-neutral item vocabulary the sync layer speaks: a
//! [`collection::Collection`] of items, each summarised by an
//! [`summary::ItemSummary`] and carrying a set of [`flag::Flag`]s.
//!
//! These are the least-common-denominator shapes above the client seam
//! (`crate::client`); the per-backend adapters that produce them keep
//! their own protocol vocabulary (an IMAP mailbox stays a mailbox inside
//! `crate::imap`) and convert at the edge.
//!
//! [`summary::ItemSummary`] and [`address::Address`] are still
//! mail-shaped: they carry `Message-ID`, subject and RFC 5322 addresses.
//! The kind seam (change `generic-pim-sync`, phase 2) moves them under
//! the `message/rfc822` kind and replaces this module's summary with a
//! per-kind one.

pub mod address;
pub mod collection;
pub mod flag;
pub mod summary;
