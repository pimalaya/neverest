//! # Item vocabulary
//!
//! The kind-neutral shapes above the client seam: a
//! [`collection::Collection`] of items, each summarised by an
//! [`summary::ItemSummary`] and carrying a set of [`flag::Flag`]s.
//!
//! Strict least-common-denominator: each adapter keeps its own protocol
//! vocabulary behind the seam (an IMAP mailbox stays a mailbox) and
//! converts at the edge. The summary and [`address::Address`] are still
//! mail-shaped, and move under the `message/rfc822` kind in phase 2.

pub mod address;
pub mod collection;
pub mod flag;
pub mod summary;
