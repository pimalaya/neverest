//! # JSON Schema registry
//!
//! Maps a CLI-invocation key, the command path joined with hyphens and
//! prefixed `neverest-`, to the JSON Schema of that command's `--json`
//! payload. [`JsonSchemaCommand`] writes one file per entry. The
//! `neverest-error` key describes the failure object instead.
//!
//! Every entry is unconditional: the backends are feature-gated but the
//! output shapes are not, so a build with no backend describes the same
//! payloads as one with all of them.
//!
//! [`JsonSchemaCommand`]: pimalaya_cli::clap::commands::JsonSchemaCommand

use std::collections::BTreeMap;

use pimalaya_cli::{clap::commands::JsonSchemaRegistry, error::ErrorReport};
use schemars::schema_for;

/// Builds the command-to-schema registry consumed by `json-schema <DIR>`.
///
/// Each value describes the type the command hands to the printer. The
/// commands confirming rather than reporting are absent: they print a
/// message and have no payload to describe. `neverest-error` describes the
/// failure object every command prints instead.
pub fn registry() -> JsonSchemaRegistry {
    let mut schemas = BTreeMap::new();

    macro_rules! insert {
        ($key:expr, $ty:ty) => {
            schemas.insert(
                $key.to_string(),
                serde_json::to_value(schema_for!($ty)).unwrap(),
            );
        };
    }

    insert!("neverest-check", crate::cli::check::CheckOutput);
    insert!("neverest-drain", crate::cli::drain::DrainOutput);
    #[cfg(feature = "wizard")]
    insert!("neverest-configure", crate::cli::configure::ConfigureOutput);
    insert!("neverest-init", crate::cli::init::InitOutput);
    insert!("neverest-sync", crate::sync::report::SyncOutput);
    insert!(
        "neverest-conflict-list",
        crate::conflict::report::ConflictListOutput
    );
    insert!(
        "neverest-conflict-show",
        crate::conflict::report::ConflictShowOutput
    );
    insert!(
        "neverest-conflict-resolve",
        crate::conflict::report::ConflictResolveOutput
    );

    insert!("neverest-error", ErrorReport);

    JsonSchemaRegistry {
        bin: "neverest",
        version: env!("CARGO_PKG_VERSION"),
        schemas,
    }
}

#[cfg(test)]
mod tests {
    use clap::{Command, CommandFactory};

    use super::registry;
    use crate::cli::main::Cli;

    /// A key naming no command is a schema nobody can ask for, which is what
    /// renaming a subcommand and forgetting the registry leaves behind. The
    /// failure object belongs to every command, so its key names none.
    #[test]
    fn every_registered_key_names_a_command() {
        let mut paths = Vec::new();
        collect(&Cli::command(), String::new(), &mut paths);

        for cmd in registry().schemas.keys() {
            if cmd == "neverest-error" {
                continue;
            }

            assert!(paths.contains(cmd), "{cmd} names no command");
        }
    }

    #[test]
    fn every_payload_is_an_object() {
        assert_eq!(registry().non_object_roots(), Vec::<&str>::new());
    }

    /// Walks the parser, naming each subcommand by its full path joined with
    /// hyphens, as the registry keys it.
    fn collect(command: &Command, prefix: String, paths: &mut Vec<String>) {
        let path = match prefix.is_empty() {
            true => command.get_name().to_string(),
            false => format!("{prefix}-{name}", name = command.get_name()),
        };

        for sub in command.get_subcommands() {
            collect(sub, path.clone(), paths);
        }

        paths.push(path);
    }
}
