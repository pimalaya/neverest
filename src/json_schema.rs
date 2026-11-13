//! # JSON Schema registry
//!
//! Maps a CLI-invocation key, the command path joined with hyphens and
//! prefixed `neverest-`, to the JSON Schema of that command's `--json`
//! payload, and to the schema of the events it prints before it.
//! [`JsonSchemaCommand`] writes one file per entry. The `neverest-error` key
//! describes the failure object instead.
//!
//! Every entry is unconditional: the backends are feature-gated but the
//! output shapes are not, so a build with no backend describes the same
//! payloads as one with all of them.
//!
//! [`JsonSchemaCommand`]: pimalaya_cli::clap::commands::json_schema::JsonSchemaCommand

use pimalaya_cli::{clap::commands::json_schema::JsonSchemaRegistry, error::ErrorReport};

use crate::{
    cli::{check::CheckRunOutput, drain::DrainRunOutput, init::InitRunOutput},
    conflict::report::{ConflictListOutput, ConflictResolveOutput, ConflictShowOutput},
    error::ErrorCode,
    sync::{event::SyncEvent, report::SyncRunOutput},
};

/// Builds the command-to-schema registry consumed by `json-schema <DIR>`.
///
/// Each result value describes the type the command hands to the printer,
/// each event value the type of the events it prints before.
/// `neverest-error` describes the failure object every command prints
/// instead.
pub fn registry() -> JsonSchemaRegistry {
    let mut registry = JsonSchemaRegistry::new("neverest", env!("CARGO_PKG_VERSION"));

    registry.insert::<CheckRunOutput>("neverest-check");
    registry.insert::<DrainRunOutput>("neverest-drain");
    #[cfg(feature = "search")]
    registry.insert::<crate::cli::index::IndexRunOutput>("neverest-index");
    registry.insert::<InitRunOutput>("neverest-init");
    registry.insert::<SyncRunOutput>("neverest-sync");
    registry.insert_events::<SyncEvent>("neverest-sync");
    registry.insert::<ConflictListOutput>("neverest-conflict-list");
    registry.insert::<ConflictShowOutput>("neverest-conflict-show");
    registry.insert::<ConflictResolveOutput>("neverest-conflict-resolve");

    registry.insert::<ErrorReport<ErrorCode>>("neverest-error");

    registry
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::registry;
    use crate::cli::main::Cli;

    /// The commands printing nothing a schema could describe: a bare
    /// `neverest` shows the help.
    const SILENT: &[&str] = &["neverest"];

    /// Every runnable command is described, and every key names one: a
    /// renamed subcommand cannot leave a schema nobody can ask for, nor a
    /// new one print undescribed data.
    #[test]
    fn the_registry_matches_the_command_tree() {
        assert_eq!(
            registry().check(&Cli::command(), SILENT),
            Vec::<String>::new()
        );
    }

    #[test]
    fn every_payload_is_an_object() {
        assert_eq!(registry().non_object_roots(), Vec::<String>::new());
    }
}
