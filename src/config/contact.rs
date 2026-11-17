//! # Contact configuration
//!
//! The `contact` table of an account: what applies to its contacts
//! whichever source holds them.

use pimalaya_config::command::CommandConfig;
use serde::{Deserialize, Serialize};

/// What applies to an account's contacts.
///
/// Whether a run merges is not a setting: the three-way merge
/// ([`crate::conflict::merge`]) is pure and fixed, so it resolves only what
/// nobody disagreed about.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ContactConfig {
    /// The merger `conflict resolve --interactive` runs on a card.
    ///
    /// Unset by default; a sync never runs it. The four paths are appended
    /// git-mergetool style, unless the command names {base}, {local}, {remote}
    /// or {output}: `tcard merge {base} {local} {remote} --output {output}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merger: Option<CommandConfig>,
}
