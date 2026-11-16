//! # Conflict configuration
//!
//! The `conflict` table of an account: how a run announces a content
//! conflict it could not merge away.

use pimalaya_config::command::CommandConfig;
use serde::{Deserialize, Serialize};

/// The only part of conflict handling anybody configures.
///
/// Whether a run merges is not a setting: the three-way merge
/// ([`crate::conflict::merge`]) is pure and fixed, so it resolves only what
/// nobody disagreed about.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ConflictConfig {
    /// The merger `conflict resolve --interactive` runs.
    ///
    /// Unset by default; a sync never runs it. The four paths are appended
    /// git-mergetool style, unless the command names {base}, {local}, {remote}
    /// or {output}: `tcard merge {base} {local} {remote} --output {output}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merger: Option<CommandConfig>,
}
