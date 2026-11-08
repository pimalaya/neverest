//! # Error codes
//!
//! A stable, machine-readable name for the failures a caller is expected
//! to act on, printed as `code` beside the message under `--json`.
//!
//! The message stays for humans and may be reworded at any time; the code
//! is the contract. A failure with no code prints as before, with no `code`
//! field. The mechanism (`CodedError`, `code_of`, the report) lives in
//! `pimalaya_cli::error`; only the vocabulary is neverest's.
//!
//! A store failure takes the code io-pimdir gives it (`PimdirError::code`),
//! spelled the same, so a caller reading every pimdir app reads one
//! vocabulary. Those neverest cannot meet are left out: `index-busy`, which
//! the indexer answers as busy; `index-missing`, `index-outdated` and
//! `index-query-only`, which answer a query client, which neverest never
//! is; `index-zone-unknown`, neverest stating only a zone its time zone
//! database named; `cutoff-invalid`, neverest writing its purge cutoff as
//! the store stamps; and `conflict-unread`, which the one-way overwrite
//! answers by fetching the unread body, an item still unread after it
//! staying parked.
//!
//! One store code is neverest's own, `store-layout`: a store laid out as
//! an earlier neverest kept it, which no pimdir profile refuses.

use io_pimdir::client::PimdirError;
use pimalaya_cli::error::CodedError;
use schemars::JsonSchema;
use serde::Serialize;

/// The stable codes, each serialised in kebab-case.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    /// The store is of another format version than neverest's (pimdir
    /// STORAGE §4.2, §6), never migrated: one an earlier neverest wrote is
    /// synced by that neverest until nothing is queued, then its directory
    /// moved away; one a newer neverest wrote asks for an upgrade.
    StoreVersion,
    /// The store is laid out as an earlier neverest kept it, never
    /// migrated: one store per account under the neverest directory, a
    /// sidecar of an earlier shape, or a database with none. Each such
    /// store is synced by that neverest until nothing is queued, then moved
    /// away.
    StoreLayout,
    /// The store has no schema yet: a sync creates it.
    StoreUncreated,
    /// The store's database is another program's, never written: the
    /// account's `store.root` names the wrong directory.
    StoreForeign,
    /// The store's version records disagree.
    StoreCorrupt,
    /// The store is not at neverest's schema: a sync brings it there.
    StoreNotCurrent,
    /// The store names its bodies with a hash neverest does not compute.
    StoreHashAlgo,
    /// Another engine version wrote the store's sync state (pimdir SYNC
    /// §10): the store is moved away and synced anew.
    SyncVersion,
    /// Another writer holds the store's write lock: retry.
    StoreBusy,
    /// Another sync owns the store: retry once it ends.
    StoreOwned,
    /// A producer is staging a body in the store: retry.
    StoreStaging,
    /// A store verb is between two of its chunks: retry.
    StoreInFlight,
    /// Another user can read or write the store, or a link stands where a
    /// lock file belongs (pimdir STORAGE §18): restrict it to its user.
    StoreExposed,
    /// A queued action's payload is malformed (pimdir STORAGE §15.3): its
    /// producer wrote what no schema reads.
    ActionMalformed,
    /// A flag the store cannot hold (pimdir STORAGE §13): empty, or holding
    /// a space, a control or a non-ASCII character.
    FlagInvalid,
    /// A source of the store does not support the write: nothing was
    /// staged.
    Unsupported,
    /// The SQLite neverest was built with has no full-text search: a
    /// `vendored` build bundles one that has.
    IndexFts5Missing,
    /// The search index is of another shape, or of a later version, than
    /// neverest's (pimdir SEARCH §2), left as it is: one of its shape at an
    /// earlier version is rebuilt instead.
    IndexVersion,
}

/// The store codes neverest relays, by io-pimdir's spelling, which
/// serialises each.
const PIMDIR_CODES: &[(&str, ErrorCode)] = &[
    ("store-version", ErrorCode::StoreVersion),
    ("store-uncreated", ErrorCode::StoreUncreated),
    ("store-foreign", ErrorCode::StoreForeign),
    ("store-corrupt", ErrorCode::StoreCorrupt),
    ("store-not-current", ErrorCode::StoreNotCurrent),
    ("store-hash-algo", ErrorCode::StoreHashAlgo),
    ("sync-version", ErrorCode::SyncVersion),
    ("store-busy", ErrorCode::StoreBusy),
    ("store-owned", ErrorCode::StoreOwned),
    ("store-staging", ErrorCode::StoreStaging),
    ("store-in-flight", ErrorCode::StoreInFlight),
    ("store-exposed", ErrorCode::StoreExposed),
    ("action-malformed", ErrorCode::ActionMalformed),
    ("flag-invalid", ErrorCode::FlagInvalid),
    ("unsupported", ErrorCode::Unsupported),
    ("index-fts5-missing", ErrorCode::IndexFts5Missing),
    ("index-version", ErrorCode::IndexVersion),
];

impl ErrorCode {
    /// The code of a store failure, `None` for one the caller cannot act
    /// on (an SQL or I/O failure).
    pub fn of_pimdir(err: &PimdirError) -> Option<Self> {
        let code = err.code();
        PIMDIR_CODES
            .iter()
            .find(|(pimdir, _)| *pimdir == code)
            .map(|(_, ours)| *ours)
    }
}

/// A store laid out as an earlier neverest kept it, refused with `message`.
pub fn layout_error(message: impl Into<String>) -> anyhow::Error {
    CodedError::new(ErrorCode::StoreLayout, message.into()).into()
}

/// A store failure as an error carrying its code, when it has one.
///
/// The message stays io-pimdir's: the caller adds its own context.
pub fn pimdir_error(err: PimdirError) -> anyhow::Error {
    match ErrorCode::of_pimdir(&err) {
        Some(code) => CodedError::new(code, err.to_string()).into(),
        None => err.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relayed_code_keeps_io_pimdirs_spelling() {
        for (pimdir, ours) in PIMDIR_CODES {
            assert_eq!(serde_json::to_value(ours).unwrap(), *pimdir);
        }
    }

    #[test]
    fn a_store_failure_carries_its_code_under_context() {
        let err = pimdir_error(PimdirError::Busy).context("Open imap store");

        assert_eq!(
            pimalaya_cli::error::code_of::<ErrorCode>(&err),
            Some(ErrorCode::StoreBusy)
        );
        assert!(format!("{err:#}").contains("write lock"), "{err:#}");
    }
}
