//! # Conflict output
//!
//! What the conflict command prints: `Display` for the terminal, `Serialize`
//! for `--json`, one type per verb.

use std::fmt;

use io_pimdir::capability::PimdirPartial;
use pimalaya_cli::output::Bytes;
use schemars::JsonSchema;
use serde::Serialize;

use crate::conflict::{Conflict, Sides};

/// What `neverest conflict list` reports: every divergence the store holds
/// for the accounts the run covers, or the fact that it is holding none.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConflictListOutput {
    /// The divergences, by account then collection then item then source.
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
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConflictSummary {
    /// The item's public id, which is what the show and resolve verbs take:
    /// an opaque string, as every Pimalaya binary prints a store id.
    pub id: String,
    /// The account whose collection holds the item.
    pub account: String,
    /// The store collection the item sits in.
    pub collection: String,
    /// The source whose own sync is stuck on the divergence, as the
    /// account's configuration names it.
    pub source: String,
    /// The item's handle on that source.
    pub handle: String,
    /// The remote revision the divergence was recorded at.
    pub revision: Option<String>,
    /// Whether a decision can be made about it at all.
    ///
    /// A conflict whose diverging remote body no run has fetched yet is
    /// visible and not resolvable; the next run makes it so.
    pub resolvable: bool,
}

impl From<&Conflict> for ConflictSummary {
    fn from(conflict: &Conflict) -> Self {
        Self {
            id: conflict.id.to_string(),
            account: conflict.account.clone(),
            collection: conflict.collection.clone(),
            source: conflict.endpoint().to_owned(),
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
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConflictShowOutput {
    /// The divergence itself.
    #[serde(flatten)]
    pub conflict: ConflictSummary,
    /// The body the last sync agreed on, the merge's common ancestor.
    pub base: Option<ConflictBody>,
    /// The local side of the divergence.
    pub local: Option<ConflictBody>,
    /// The remote side, `null` until a run has fetched it.
    pub remote: Option<ConflictBody>,
}

impl ConflictShowOutput {
    /// Takes the three bodies as stored, typed by the collection's media
    /// type. Every kind reaching a conflict is text: mail alone is
    /// immutable-content, and it reaches none of this.
    pub fn new(conflict: &Conflict, sides: Sides) -> Self {
        let body =
            |body: Option<Vec<u8>>| body.map(|body| ConflictBody::new(body, &conflict.media_type));

        Self {
            conflict: conflict.into(),
            base: body(sides.base),
            local: body(sides.local),
            remote: body(sides.remote),
        }
    }
}

/// One side of a divergence: its text when the stored bytes are UTF-8,
/// else the bytes themselves, never decoded lossily.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ConflictBody {
    /// The body, valid UTF-8.
    Text(String),
    /// The body, not valid UTF-8 (a vCard 2.1 in Latin-1, for one).
    Bytes(Bytes),
}

impl ConflictBody {
    fn new(body: Vec<u8>, media_type: &str) -> Self {
        match String::from_utf8(body) {
            Ok(text) => Self::Text(text),
            Err(err) => Self::Bytes(Bytes {
                data: err.into_bytes(),
                mime: media_type.to_owned(),
            }),
        }
    }
}

impl fmt::Display for ConflictBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => writeln!(f, "{}", text.trim_end()),
            // NOTE: a person reads this to decide; --json and the store
            // keep the bytes.
            Self::Bytes(bytes) => {
                writeln!(f, "(not UTF-8, invalid sequences shown as U+FFFD)")?;
                let text = String::from_utf8_lossy(&bytes.data);
                writeln!(f, "{}", text.trim_end())
            }
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
                    write!(f, "{body}")?;
                }
                None => writeln!(f, "{name}: not in the store")?,
            }
        }

        writeln!(f)
    }
}

/// What `neverest conflict resolve <id>` concluded.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    tag = "outcome"
)]
pub enum ConflictResolveOutput {
    /// The decision was applied: the item holds the chosen body and is no
    /// longer conflicted. The next run pushes it.
    Resolved {
        /// The item that was settled, its public id.
        id: String,
        /// The store collection it sits in.
        collection: String,
        /// The side the decision took: the local body, the remote one, or
        /// the one the interactive merger wrote.
        side: String,
        /// What a source supports in part that the next run's push relies
        /// on (pimdir STORAGE §15.6): an event's attendees notified by the
        /// provider's rules rather than the event's own, for one.
        partial: Vec<PartialCapability>,
    },
    /// The merger aborted, by exiting non-zero or by leaving its output
    /// untouched, so nothing was decided and nothing was pushed.
    Aborted {
        /// The item that stays conflicted, its public id.
        id: String,
    },
}

/// A capability a source supports in part, which a settled body relies on.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartialCapability {
    /// The capability, as the store names it (pimdir STORAGE §15).
    pub capability: String,
    /// The source declaring it, as the account's configuration names it.
    pub source: String,
    /// What the source leaves out, `null` when it says nothing more.
    pub detail: Option<String>,
}

impl PartialCapability {
    /// The store's partial row, its source named as the configuration of
    /// `account` names it rather than by its store id, `<account>/<name>`.
    pub fn new(partial: PimdirPartial, account: &str) -> Self {
        let PimdirPartial {
            capability,
            source,
            detail,
        } = partial;
        let source = match source.strip_prefix(&format!("{account}/")) {
            Some(name) => name.to_owned(),
            None => source,
        };

        Self {
            capability,
            source,
            detail,
        }
    }
}

impl fmt::Display for PartialCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            capability,
            source,
            detail,
        } = self;

        write!(f, "Source {source} supports {capability} in part")?;

        if let Some(detail) = detail {
            write!(f, ": {detail}")?;
        }

        Ok(())
    }
}

impl fmt::Display for ConflictResolveOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolved {
                id,
                collection,
                side,
                partial,
            } => {
                writeln!(
                    f,
                    "Settled conflict {id} in {collection} with the {side} body"
                )?;
                for partial in partial {
                    writeln!(f, " - {partial}")?;
                }
                Ok(())
            }
            Self::Aborted { id } => writeln!(
                f,
                "The merger decided nothing, so conflict {id} is exactly as it was"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::ConflictBody;

    /// A body that is not UTF-8 prints its exact bytes, where it was
    /// decoded with U+FFFD replacements.
    #[test]
    fn a_latin1_body_prints_its_bytes() {
        let body = ConflictBody::new(b"FN:Ren\xe9\r\n".to_vec(), "text/vcard");

        assert_eq!(
            serde_json::to_value(&body).unwrap(),
            json!({"data": "Rk46UmVu6Q0K", "size": 9, "mime": "text/vcard"}),
        );
    }

    #[test]
    fn a_utf8_body_prints_as_text() {
        let body = ConflictBody::new("FN:René\r\n".as_bytes().to_vec(), "text/vcard");

        assert_eq!(serde_json::to_value(&body).unwrap(), json!("FN:René\r\n"));
    }

    /// A settled divergence names what a source supports in part as rows a
    /// program reads, its source as the configuration names it, where each
    /// was a sentence.
    #[test]
    fn a_partial_capability_prints_as_a_row() {
        use io_pimdir::capability::PimdirPartial;

        use super::{ConflictResolveOutput, PartialCapability};

        let partial = PimdirPartial {
            capability: String::from("calendar.scheduling"),
            source: String::from("work/gcal"),
            detail: Some(String::from("Google notifies the attendees")),
        };
        let output = ConflictResolveOutput::Resolved {
            id: String::from("42"),
            collection: String::from("work/gcal/primary"),
            side: String::from("local"),
            partial: vec![PartialCapability::new(partial, "work")],
        };

        assert_eq!(
            serde_json::to_value(&output).unwrap(),
            json!({
                "outcome": "resolved",
                "id": "42",
                "collection": "work/gcal/primary",
                "side": "local",
                "partial": [{
                    "capability": "calendar.scheduling",
                    "source": "gcal",
                    "detail": "Google notifies the attendees",
                }],
            }),
        );
        assert!(
            output
                .to_string()
                .contains(" - Source gcal supports calendar.scheduling in part: Google"),
            "{output}"
        );
    }
}
