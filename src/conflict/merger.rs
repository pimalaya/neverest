//! # Interactive merger
//!
//! The program a person settles a collision with. Neverest runs no editor and
//! renders no form: it hands the three bodies to a program the account names
//! and takes back one body, and what that program does with them is its own
//! business.
//!
//! Following git mergetool, the bodies travel as filesystem paths rather than
//! on standard input, base first, then the divergent sides, then the path to
//! write. They are appended positionally, and a command carrying placeholders
//! is substituted instead, for a tool with an argument shape of its own.
//!
//! The result is taken only on a zero exit with the output written: an editor
//! exits zero on a bare quit, so a zero exit alone is not a choice. Written
//! means the output file's bytes differ from the ones this put there, which
//! no clock skew and no timestamp granularity can get wrong.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use log::{debug, warn};
use pimalaya_config::command::{CommandConfig, shell};

use crate::conflict::Sides;

/// The placeholders a merger with an argument shape of its own is written
/// with, in the order the positional form appends them.
const PLACEHOLDERS: [&str; 4] = ["{base}", "{local}", "{remote}", "{output}"];

/// The bytes the output path is seeded with, and what an untouched output
/// still holds.
///
/// Empty, so a merger writing nothing and one writing an empty body are the
/// same abort, which they are: an empty card settles nothing.
const UNWRITTEN: &[u8] = b"";

/// One invocation of the configured interactive merger over one divergence.
pub struct Merger<'a> {
    /// The command the account's `conflict.merger` names.
    pub command: &'a CommandConfig,
    /// The body the last sync agreed on, the merge's common ancestor.
    pub base: PathBuf,
    /// The local side of the divergence.
    pub local: PathBuf,
    /// The remote side of the divergence.
    pub remote: PathBuf,
    /// Where the merger writes the body it settled on.
    pub output: PathBuf,
}

impl<'a> Merger<'a> {
    /// Writes the three bodies into `dir` under the kind's `extension` and
    /// names the output path beside them.
    ///
    /// A side the store does not hold is refused rather than exported as an
    /// empty file: a merger handed an empty vCard as the common ancestor
    /// would report every field as a conflict.
    pub fn export(
        command: &'a CommandConfig,
        dir: &Path,
        extension: &str,
        sides: &Sides,
    ) -> Result<Self> {
        let write = |name: &str, body: &Option<Vec<u8>>| -> Result<PathBuf> {
            let Some(body) = body else {
                bail!("The {name} side of this conflict is not in the store");
            };

            let path = dir.join(format!("{name}.{extension}"));
            fs::write(&path, body)
                .with_context(|| format!("Export the {name} side to {}", path.display()))?;

            Ok(path)
        };

        Ok(Self {
            command,
            base: write("base", &sides.base)?,
            local: write("local", &sides.local)?,
            remote: write("remote", &sides.remote)?,
            output: dir.join(format!("merged.{extension}")),
        })
    }

    /// Runs the merger and takes back the body it wrote, or `None` when it
    /// aborted by exiting non-zero or by leaving its output untouched.
    ///
    /// Standard input and output are the terminal's, which is the point: the
    /// person typed the command this runs from.
    pub fn run(&self) -> Result<Option<Vec<u8>>> {
        fs::write(&self.output, UNWRITTEN)
            .with_context(|| format!("Seed the merger output {}", self.output.display()))?;

        let mut command = self.command();
        debug!("run the interactive merger: {command:?}");

        let status = command.status().context("Run the interactive merger")?;

        if !status.success() {
            warn!("the merger exited with {status}, leaving the conflict as it was");
            return Ok(None);
        }

        let body = fs::read(&self.output)
            .with_context(|| format!("Read the merger output {}", self.output.display()))?;

        if body == UNWRITTEN {
            warn!("the merger wrote no body, leaving the conflict as it was");
            return Ok(None);
        }

        Ok(Some(body))
    }

    /// Builds the command the four paths reach the merger through: appended
    /// positionally, or substituted where the configuration named where each
    /// one goes.
    fn command(&self) -> Command {
        match self.command {
            CommandConfig::Shell(line) => {
                // A shell line is a line, so a path holding a space becomes
                // two arguments unless it is quoted here.
                let paths = self.paths(quote);

                match substitute(line, &paths) {
                    Some(line) => shell(&line),
                    None => shell(&format!("{line} {}", paths.join(" "))),
                }
            }
            CommandConfig::Argv { program, args } => {
                // No shell in between, so a path is one argument whatever it
                // holds.
                let paths = self.paths(|path| path.display().to_string());

                let mut command = Command::new(program);
                let mut substituted = false;

                for arg in args {
                    match substitute(arg, &paths) {
                        Some(arg) => {
                            substituted = true;
                            command.arg(arg);
                        }
                        None => {
                            command.arg(arg);
                        }
                    }
                }

                if !substituted {
                    command.args(paths);
                }

                command
            }
        }
    }

    /// The four paths in contract order, rendered by `render`.
    fn paths(&self, render: impl Fn(&Path) -> String) -> Vec<String> {
        [&self.base, &self.local, &self.remote, &self.output]
            .into_iter()
            .map(|path| render(path))
            .collect()
    }
}

/// Replaces every placeholder in `text` with the path beside it, or `None`
/// when the text carries none, which is what tells the two forms apart.
fn substitute(text: &str, paths: &[String]) -> Option<String> {
    if !PLACEHOLDERS
        .iter()
        .any(|placeholder| text.contains(placeholder))
    {
        return None;
    }

    let mut substituted = text.to_string();

    for (placeholder, path) in PLACEHOLDERS.iter().zip(paths) {
        substituted = substituted.replace(placeholder, path);
    }

    Some(substituted)
}

/// Quotes a path for the platform shell, so it survives whatever it holds.
#[cfg(unix)]
fn quote(path: &Path) -> String {
    let path = path.display().to_string().replace('\'', r"'\''");

    format!("'{path}'")
}

/// Quotes a path for the platform shell, so it survives whatever it holds.
#[cfg(windows)]
fn quote(path: &Path) -> String {
    let path = path.display();

    format!("\"{path}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three bodies every merger test hands over.
    fn sides() -> Sides {
        Sides {
            base: Some(b"base".to_vec()),
            local: Some(b"local".to_vec()),
            remote: Some(b"remote".to_vec()),
        }
    }

    /// A merger that refuses, and one that exits zero without writing, are
    /// the same answer. Taking the second as a decision would discard a side
    /// every time somebody quit an editor.
    #[cfg(unix)]
    #[test]
    fn a_merger_that_aborts_or_writes_nothing_yields_no_body() {
        let dir = tempfile::tempdir().unwrap();

        for line in ["false", "true", "cat {base} > /dev/null"] {
            let command = CommandConfig::Shell(String::from(line));
            let merger = Merger::export(&command, dir.path(), "vcf", &sides()).unwrap();

            assert_eq!(merger.run().unwrap(), None, "{line}");
        }
    }

    /// The positional contract: the four paths are appended base first, so a
    /// merger taking that order needs nothing but its own name.
    #[cfg(unix)]
    #[test]
    fn a_positional_merger_is_handed_the_four_paths_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let command = CommandConfig::Shell(String::from(r#"sh -c 'cat "$1" "$2" "$3" > "$4"' --"#));
        let merger = Merger::export(&command, dir.path(), "vcf", &sides()).unwrap();

        assert_eq!(merger.run().unwrap(), Some(b"baselocalremote".to_vec()));
    }

    /// The placeholder contract, for a merger whose output is a flag rather
    /// than the last argument.
    #[cfg(unix)]
    #[test]
    fn a_merger_naming_its_placeholders_is_substituted_rather_than_appended() {
        let dir = tempfile::tempdir().unwrap();
        let command = CommandConfig::Argv {
            program: String::from("cp"),
            args: vec![String::from("{remote}"), String::from("{output}")],
        };
        let merger = Merger::export(&command, dir.path(), "vcf", &sides()).unwrap();

        assert_eq!(merger.run().unwrap(), Some(b"remote".to_vec()));
    }
}
