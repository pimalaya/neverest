//! # Mail configuration
//!
//! The `mail` table of an account: what applies to its mail whichever
//! source holds it.

use serde::{Deserialize, Serialize};

use crate::config::is_default;

/// What applies to an account's mail.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MailConfig {
    /// Which messages a sync lists.
    #[serde(default, skip_serializing_if = "is_default")]
    pub filter: MailFilter,
}

/// Which messages a sync lists (pimdir SYNC §5).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MailFilter {
    /// The floor of the `Date` scope a mail collection syncs under: `30d`,
    /// `12w`, `6mo`, `1y`, a date (midnight UTC) or an RFC 3339 instant.
    ///
    /// Mail outside it is neither listed nor deleted: what a wider sync
    /// stored stays. Refused beside contacts or calendars; `sync --since`
    /// overrides it for one run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}
