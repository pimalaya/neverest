//! # Error codes
//!
//! A stable, machine-readable name for the failures a caller is expected
//! to act on, printed as `code` beside the message under `--json`.
//!
//! The message stays for humans and may be reworded at any time; the code
//! is the contract. The mechanism (`CodedError`, `code_of`, the report)
//! lives in `pimalaya_cli::error`; only the vocabulary is neverest's.
//!
//! A store failure takes io-pimdir's code, spelled the same, but the codes
//! neverest cannot meet, listed in cairn/spec/sync.md. One store code is
//! neverest's own, `store-layout`, which no pimdir profile refuses.
//!
//! A server's refusal takes a code of the vocabulary Himalaya shares, read
//! from the typed error of the protocol that answered it. A report row
//! failing carries it as a [`Failure`].

use anyhow::Error;
use io_pimdir::{client::PimdirError, mutate::PimdirMutateError};
use pimalaya_cli::error::{CodedError, code_of, with_code};
use schemars::JsonSchema;
use serde::Serialize;

/// The stable codes, each serialised in kebab-case.
#[cfg_attr(
    not(any(
        feature = "carddav",
        feature = "caldav",
        feature = "gcal",
        feature = "gmail",
        feature = "gpeople",
        feature = "imap",
        feature = "msgraph",
        feature = "smtp"
    )),
    allow(dead_code)
)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    /// The server refused the request to the account.
    ///
    /// An HTTP 403 that is no rate limit and, on DAV, names no precondition
    /// but `need-privileges` (RFC 3744 §7.1.1), or an IMAP `NOPERM` (RFC
    /// 5530).
    PermissionDenied,
    /// The write or the send would exceed the account's quota.
    ///
    /// An HTTP 507, a DAV `quota-not-exceeded` (RFC 4331 §6), an IMAP
    /// `OVERQUOTA` (RFC 9208), Graph's `ErrorQuotaExceeded`, an SMTP reply
    /// naming a sending limit.
    QuotaExceeded,
    /// The server still throttled the request past the waits neverest
    /// takes: an HTTP 429, or a Google rate limit or daily quota.
    RateLimited,
    /// The SMTP server refused a recipient of a send (RFC 5321 §3.3).
    ///
    /// The message reached nobody: io-smtp offers none after the first
    /// refused.
    RecipientRefused,
    /// The store is of another format version (pimdir STORAGE §4.2, §6).
    ///
    /// Never migrated: one an earlier neverest wrote is synced by that
    /// neverest until nothing is queued, then its directory moved away; one
    /// a newer neverest wrote asks for an upgrade.
    StoreVersion,
    /// The store is laid out as an earlier neverest kept it, never migrated.
    ///
    /// One store per account, a sidecar of an earlier shape, or a database
    /// with none: each is synced by that neverest until nothing is queued,
    /// then moved away.
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
    /// A store lock file was replaced while held (pimdir STORAGE §8).
    ///
    /// Another process may own the store: nothing more was written, and the
    /// next run takes the lock anew.
    LockReplaced,
    /// A queued action's payload is malformed (pimdir STORAGE §15.3): its
    /// producer wrote what no schema reads.
    ActionMalformed,
    /// A flag the store cannot hold (pimdir STORAGE §13): empty, or holding
    /// a space, a control or a non-ASCII character.
    FlagInvalid,
    /// A source of the store does not support the write: nothing was
    /// staged.
    Unsupported,
    /// The store refused an empty source id, which no row names: nothing
    /// was written.
    SourceEmpty,
    /// A copy or a move names a collection no source syncs, where no create
    /// can be staged (pimdir SYNC §7): nothing was staged.
    TargetUnsynced,
    /// The SQLite neverest was built with has no full-text search: a
    /// `vendored` build bundles one that has.
    IndexFts5Missing,
    /// The search index is of another shape or a later version (SEARCH §2).
    ///
    /// It is left as it is: one of its shape at an earlier version is
    /// rebuilt instead.
    IndexVersion,
    /// No account has a name `-a` gives.
    AccountMissing,
    /// The account was never initialized in its store: `neverest init`, or
    /// `sync --init`, initializes it.
    AccountUninitialized,
    /// No conflict of the account has the id given.
    ConflictNotFound,
    /// A side refused a create because the collection already holds an
    /// item of its `UID` (RFC 4791 §5.3.2, RFC 6352 §6.3.2).
    ItemDuplicate,
    /// A source's backend is one this build leaves out: rebuild with the
    /// cargo feature the message names.
    BackendNotBuilt,
}

/// The store codes neverest relays, by io-pimdir's spelling
/// (`PimdirError::code`, `PimdirMutateError::code`), which serialises each.
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
    ("lock-replaced", ErrorCode::LockReplaced),
    ("action-malformed", ErrorCode::ActionMalformed),
    ("flag-invalid", ErrorCode::FlagInvalid),
    ("unsupported", ErrorCode::Unsupported),
    ("source-empty", ErrorCode::SourceEmpty),
    ("target-unsynced", ErrorCode::TargetUnsynced),
    ("index-fts5-missing", ErrorCode::IndexFts5Missing),
    ("index-version", ErrorCode::IndexVersion),
];

impl ErrorCode {
    /// The code of a store failure, `None` for one the caller cannot act
    /// on (an SQL or I/O failure).
    pub fn of_pimdir(err: &PimdirError) -> Option<Self> {
        Self::relayed(err.code())
    }

    /// The code of a local edit the store refused, `None` for one the
    /// caller cannot act on.
    pub fn of_mutate(err: &PimdirMutateError) -> Option<Self> {
        Self::relayed(err.code())
    }

    /// The code neverest relays for io-pimdir's `code`.
    fn relayed(code: &str) -> Option<Self> {
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

/// A local edit the store refused as an error carrying its code, when it
/// has one.
pub fn mutate_error(err: PimdirMutateError) -> anyhow::Error {
    match ErrorCode::of_mutate(&err) {
        Some(code) => CodedError::new(code, err.to_string()).into(),
        None => err.into(),
    }
}

/// A failure as a report row carries it: pimalaya-cli's item error less
/// its id, the row naming its item itself.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Failure {
    /// The stable code, `None` for a cause no caller is expected to act on.
    pub code: Option<ErrorCode>,
    /// The outermost message.
    pub error: String,
    /// The messages of the causes, outermost first.
    pub sources: Vec<String>,
}

impl Failure {
    /// The failure `err` reads as, coded by [`code`].
    pub fn of(err: &Error) -> Self {
        Self {
            code: code(err),
            error: err.to_string(),
            sources: err.chain().skip(1).map(ToString::to_string).collect(),
        }
    }

    /// The error this failure was read from, its messages and code kept,
    /// the code carried by the innermost message.
    pub fn to_error(&self) -> Error {
        let mut messages: Vec<&str> = Some(self.error.as_str())
            .into_iter()
            .chain(self.sources.iter().map(String::as_str))
            .collect();
        let innermost = messages.pop().unwrap_or_default().to_owned();
        let err = match self.code {
            Some(code) => Error::new(CodedError::new(code, innermost)),
            None => Error::msg(innermost),
        };

        messages
            .into_iter()
            .rev()
            .fold(err, |err, message| err.context(message.to_owned()))
    }

    /// A failure neverest states in its own words, with no code and no
    /// cause.
    pub fn new(error: impl Into<String>) -> Self {
        Self {
            code: None,
            error: error.into(),
            sources: Vec::new(),
        }
    }

    /// The `code`, `error` and `sources` of a row that may have
    /// succeeded, `None` reading as a success.
    pub fn row(failure: Option<Self>) -> (Option<ErrorCode>, Option<String>, Vec<String>) {
        match failure {
            Some(Self {
                code,
                error,
                sources,
            }) => (code, Some(error), sources),
            None => (None, None, Vec::new()),
        }
    }
}

impl From<String> for Failure {
    fn from(error: String) -> Self {
        Self::new(error)
    }
}

impl From<&str> for Failure {
    fn from(error: &str) -> Self {
        Self::new(error)
    }
}

/// A failure's message followed by each of its causes, as `{err:#}` reads.
pub fn chain(error: &str, sources: &[String]) -> String {
    let mut text = String::from(error);
    for source in sources {
        text.push_str(": ");
        text.push_str(source);
    }
    text
}

/// The code of `err`: the one a coded error in its chain carries, else
/// the server refusal's under it.
pub fn code(err: &Error) -> Option<ErrorCode> {
    code_of(err).or_else(|| refusal_code(err))
}

/// `err` as the report reads it: the server refusal's code under it carried
/// by an innermost coded error, its messages kept (see [`with_code`]).
pub fn coded(err: Error) -> Error {
    with_code(err, refusal_code)
}

/// The reason a server refused a change for good, `err`'s messages joined,
/// `None` when nothing in its chain says so (pimdir SYNC §4).
///
/// A coded `unsupported` or a refusal naming the request refuses it, else
/// the next run pushes it again. A quota or a rate limit never does: the
/// account's state, not the change's, which a refusal would hold for good.
pub fn refusal(err: &Error) -> Option<String> {
    if matches!(
        code(err),
        Some(ErrorCode::QuotaExceeded | ErrorCode::RateLimited)
    ) {
        return None;
    }

    let refusers: &[Refuses] = &[
        #[cfg(any(feature = "carddav", feature = "caldav"))]
        crate::dav::client::refuses,
        #[cfg(feature = "gcal")]
        crate::gcal::client::refuses,
        #[cfg(feature = "gmail")]
        crate::gmail::client::refuses,
        #[cfg(feature = "gpeople")]
        crate::gpeople::client::refuses,
        #[cfg(feature = "imap")]
        crate::imap::backend::refuses,
        #[cfg(feature = "msgraph")]
        crate::msgraph::client::refuses,
    ];
    let refused = code_of(err) == Some(ErrorCode::Unsupported)
        || err
            .chain()
            .any(|cause| refusers.iter().any(|refuses| refuses(cause)));

    refused.then(|| format!("{err:#}"))
}

/// Whether an HTTP status names the request itself (pimdir SYNC §4).
///
/// Any from 400 to 499 but those naming the credentials (401, 407), a
/// state that passes (408, 423, 425, 429) or a stale base (409, 412).
#[cfg_attr(
    not(any(
        feature = "carddav",
        feature = "caldav",
        feature = "gcal",
        feature = "gmail",
        feature = "gpeople",
        feature = "msgraph"
    )),
    allow(dead_code)
)]
pub(crate) fn names_the_request(status: u16) -> bool {
    (400..500).contains(&status) && !matches!(status, 401 | 407 | 408 | 409 | 412 | 423 | 425 | 429)
}

/// A reader of one protocol's typed errors, telling a refusal of the
/// change as it stands ([`refusal`]).
type Refuses = fn(&(dyn std::error::Error + 'static)) -> bool;

/// A reader of one protocol's typed errors, coding the refusals it knows.
type Refusal = fn(&(dyn std::error::Error + 'static)) -> Option<ErrorCode>;

/// The code of the server refusal `err`'s chain carries: the first cause
/// the typed error of the protocol that answered it codes.
fn refusal_code(err: &Error) -> Option<ErrorCode> {
    let refusals: &[Refusal] = &[
        #[cfg(any(feature = "carddav", feature = "caldav"))]
        crate::dav::client::refusal_code,
        #[cfg(feature = "gcal")]
        crate::gcal::client::refusal_code,
        #[cfg(feature = "gmail")]
        crate::gmail::client::refusal_code,
        #[cfg(feature = "gpeople")]
        crate::gpeople::client::refusal_code,
        #[cfg(feature = "imap")]
        crate::imap::backend::refusal_code,
        #[cfg(feature = "msgraph")]
        crate::msgraph::client::refusal_code,
        #[cfg(feature = "smtp")]
        crate::sync::submit::refusal_code,
    ];

    err.chain()
        .find_map(|cause| refusals.iter().find_map(|refusal| refusal(cause)))
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

    /// A 4xx names the request but for credentials, a passing state or a
    /// stale base (pimdir SYNC §4); a server failure never does.
    #[test]
    fn a_status_names_the_request_but_a_few() {
        for status in [400, 403, 404, 405, 410, 413, 415, 422] {
            assert!(names_the_request(status), "{status}");
        }
        for status in [
            200, 302, 401, 407, 408, 409, 412, 423, 425, 429, 500, 503, 507,
        ] {
            assert!(!names_the_request(status), "{status}");
        }
    }

    /// A coded `unsupported` refuses under any context; a quota or a rate
    /// limit never does, and an uncoded failure cannot tell.
    #[test]
    fn an_unsupported_change_is_refused_and_a_quota_is_not() {
        let unsupported = Error::new(CodedError::new(
            ErrorCode::Unsupported,
            String::from("Graph appends to Drafts only"),
        ))
        .context("Append to Archive");
        let reason = refusal(&unsupported).expect("refused");
        assert_eq!(reason, "Append to Archive: Graph appends to Drafts only");

        let quota = Error::new(CodedError::new(
            ErrorCode::QuotaExceeded,
            String::from("full"),
        ));
        assert_eq!(refusal(&quota), None);
        assert_eq!(refusal(&anyhow::anyhow!("connection reset")), None);
    }

    /// A lock file replaced under a run is a failure its caller can act
    /// on, by running again: it carries io-pimdir's code.
    #[test]
    fn a_replaced_lock_carries_its_code() {
        let err = pimdir_error(PimdirError::LockReplaced("owner.lock".into()));

        assert_eq!(
            pimalaya_cli::error::code_of::<ErrorCode>(&err),
            Some(ErrorCode::LockReplaced)
        );
    }

    /// A copy or a move into a collection no source syncs carries
    /// io-pimdir's code, and so does an empty source id.
    #[test]
    fn an_unsynced_target_and_an_empty_source_carry_their_codes() {
        let unsynced = mutate_error(PimdirMutateError::Unsynced("work/Archive".into()));
        let empty = pimdir_error(PimdirError::SourceEmpty);

        assert_eq!(
            pimalaya_cli::error::code_of::<ErrorCode>(&unsynced),
            Some(ErrorCode::TargetUnsynced)
        );
        assert_eq!(
            pimalaya_cli::error::code_of::<ErrorCode>(&empty),
            Some(ErrorCode::SourceEmpty)
        );
    }
}
