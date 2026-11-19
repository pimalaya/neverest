//! # Calendar configuration
//!
//! The `calendar` table of the document or an account: what applies to its
//! calendars whichever source holds them.

use pimalaya_config::command::CommandConfig;
use serde::{Deserialize, Serialize};

/// What applies to an account's calendars.
///
/// Whether a run merges is not a setting: the three-way merge
/// ([`crate::conflict::merge`]) is pure and fixed, so it resolves only what
/// nobody disagreed about.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct CalendarConfig {
    /// The merger `conflict resolve --interactive` runs on an event.
    ///
    /// Unset takes the document's; a sync never runs it. The paths are
    /// appended unless the command names {base}, {local}, {remote} or
    /// {output}: `tcal merge {base} {local} {remote} --output {output}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merger: Option<CommandConfig>,
}
