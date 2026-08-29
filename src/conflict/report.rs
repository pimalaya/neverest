//! What the conflict command prints: `Display` for the terminal, `Serialize`
//! for `--json`, one type per verb.

use std::fmt;

use serde::Serialize;

use crate::conflict::{Conflict, Sides};

/// What `neverest conflict list` reports: every divergence the account's
/// store is holding, or the fact that it is holding none.
#[derive(Debug, Serialize)]
pub struct ConflictListOutput {
    /// The divergences, by collection then item then source.
    pub conflicts: Vec<ConflictSummary>,
}

impl fmt::Display for ConflictListOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f)?;

        if self.conflicts.is_empty() {
            return writeln!(f, "No item is waiting for a decision");
        }

        writeln!(
            f,
            "Conflicts ({n} item(s) waiting for a decision):",
            n = self.conflicts.len()
        )?;

        for conflict in &self.conflicts {
            writeln!(f, " - {conflict}")?;
        }

        writeln!(f)
    }
}

/// One divergence as a listing names it.
#[derive(Debug, Serialize)]
pub struct ConflictSummary {
    /// The item's public id, which is what the show and resolve verbs take.
    pub id: i64,
    /// The store collection the item sits in.
    pub collection: String,
    /// The source whose own sync is stuck on the divergence.
    pub source: String,
    /// The item's handle on that source.
    pub handle: String,
    /// The remote revision the divergence was recorded at.
    pub revision: Option<String>,
    /// Whether a decision can be made about it at all. A conflict whose
    /// diverging remote body a run has not fetched yet is visible and not
    /// resolvable, and the next run makes it so.
    pub resolvable: bool,
}

impl From<&Conflict> for ConflictSummary {
    fn from(conflict: &Conflict) -> Self {
        Self {
            id: conflict.id,
            collection: conflict.collection.clone(),
            source: conflict.source.clone(),
            handle: conflict.handle.clone(),
            revision: conflict.revision.clone(),
            resolvable: conflict.resolvable(),
        }
    }
}

impl fmt::Display for ConflictSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            id,
            collection,
            source,
            handle,
            resolvable,
            ..
        } = self;

        write!(f, "{id} in {collection} on {source} ({handle})")?;

        if !resolvable {
            write!(f, ", waiting for its diverging body")?;
        }

        Ok(())
    }
}

/// What `neverest conflict show <id>` reports: the divergence and the three
/// bodies it is between, which is what a decision is made from.
#[derive(Debug, Serialize)]
pub struct ConflictShowOutput {
    /// The divergence itself.
    #[serde(flatten)]
    pub conflict: ConflictSummary,
    /// The body the last sync agreed on, the merge's common ancestor.
    pub base: Option<String>,
    /// The local side of the divergence.
    pub local: Option<String>,
    /// The remote side, absent until a run has fetched it.
    pub remote: Option<String>,
}

impl ConflictShowOutput {
    /// Renders the three bodies as text, which every kind reaching a conflict
    /// is: mail alone is immutable-content, and it reaches none of this.
    pub fn new(conflict: &Conflict, sides: Sides) -> Self {
        let text =
            |body: Option<Vec<u8>>| body.map(|body| String::from_utf8_lossy(&body).into_owned());

        Self {
            conflict: conflict.into(),
            base: text(sides.base),
            local: text(sides.local),
            remote: text(sides.remote),
        }
    }
}

impl fmt::Display for ConflictShowOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f)?;
        writeln!(f, "Conflict {conflict}", conflict = self.conflict)?;

        for (name, body) in [
            ("Base", &self.base),
            ("Local", &self.local),
            ("Remote", &self.remote),
        ] {
            writeln!(f)?;

            match body {
                Some(body) => {
                    writeln!(f, "{name}:")?;
                    writeln!(f, "{}", body.trim_end())?;
                }
                None => writeln!(f, "{name}: not in the store")?,
            }
        }

        writeln!(f)
    }
}

/// What `neverest conflict resolve <id>` concluded.
#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case", tag = "outcome")]
pub enum ConflictResolveOutput {
    /// The decision was applied: the item holds the chosen body and is no
    /// longer conflicted. The next run pushes it, conditioned on the
    /// revision the divergence was recorded at.
    Resolved {
        /// The item that was settled.
        id: i64,
        /// The store collection it sits in.
        collection: String,
        /// The side the decision took: the local body, the remote one, or
        /// the one the interactive merger wrote.
        side: String,
    },
    /// The merger aborted, by exiting non-zero or by leaving its output
    /// untouched, so nothing was decided and nothing was pushed.
    Aborted {
        /// The item that stays conflicted.
        id: i64,
    },
}

impl fmt::Display for ConflictResolveOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolved {
                id,
                collection,
                side,
            } => writeln!(
                f,
                "Settled conflict {id} in {collection} with the {side} body"
            ),
            Self::Aborted { id } => writeln!(
                f,
                "The merger decided nothing, so conflict {id} is exactly as it was"
            ),
        }
    }
}
