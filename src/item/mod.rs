//! # Item vocabulary
//!
//! The kind-neutral shapes above the client seam: a
//! [`collection::Collection`] of items, each carrying a set of
//! [`flag::Flag`]s. What summarises an item is io-pimdir's own row
//! (pimdir STORAGE Annex A), which each adapter derives at its edge.
//!
//! Strict least-common-denominator: each adapter keeps its own protocol
//! vocabulary behind the seam (an IMAP mailbox stays a mailbox) and
//! converts at the edge.

pub mod collection;
pub mod flag;
