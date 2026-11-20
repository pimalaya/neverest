//! # Account mode
//!
//! What an account does, read off its configuration: which endpoints, which
//! way, whether bodies stay.

use std::{collections::HashMap, fmt};

use schemars::JsonSchema;
use serde::Serialize;

use crate::config::source::SourceConfig;

/// What an account does: which endpoints, which direction, whether bodies stay.
///
/// Declared, never derived: the mode is the arity of `sources` and
/// `targets` plus the two flags, so no behaviour depends on a coincidence
/// between two sources.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountMode {
    /// The source names, sorted, so a report reads the same twice.
    pub sources: Vec<String>,
    /// The target names, sorted; empty means the store is the destination.
    pub targets: Vec<String>,
    /// Whether the `sources` side is authoritative.
    pub one_way: bool,
    /// Whether the store holds bodies, resolved from the destination when
    /// the configuration left it unset.
    pub retain: bool,
}

impl AccountMode {
    /// Whether the local store is the destination rather than a remote.
    pub fn is_local(&self) -> bool {
        self.targets.is_empty()
    }

    /// Whether a crossing may be streamed rather than staged in the store.
    ///
    /// An internal choice, not a mode: what the user declared is `retain`,
    /// which both answers honour. It needs both endpoints on a protocol that
    /// takes a body straight from the other, so IMAP to IMAP today.
    pub fn streams(&self, sources: &HashMap<String, SourceConfig>) -> bool {
        !self.retain
            && !self.is_local()
            && self
                .sources
                .iter()
                .chain(&self.targets)
                .all(|name| sources.get(name).is_some_and(SourceConfig::is_streamable))
    }
}

impl fmt::Display for AccountMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sources = listed(&self.sources);

        if self.is_local() {
            let verb = match (self.one_way, self.sources.len()) {
                (true, 1) => "overwrites the local store, discarding local edits",
                (true, _) => "each overwrite the local store, discarding local edits",
                (false, 1) => "syncs both ways with the local store",
                (false, _) => "each sync both ways with the local store",
            };

            return write!(f, "{sources} {verb}");
        }

        let targets = listed(&self.targets);
        let body = if self.retain {
            ", keeping a local copy"
        } else {
            ", keeping no local copy"
        };

        if self.one_way {
            write!(f, "{sources} overwrites {targets}{body}")
        } else {
            write!(f, "{sources} and {targets} sync both ways{body}")
        }
    }
}

/// `names` as a sentence lists them: `a`, `a and b`, `a, b and c`.
fn listed(names: &[String]) -> String {
    match names.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, others)) => format!("{} and {last}", others.join(", ")),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use crate::config::AccountConfig;

    /// The sentence agrees with how many endpoints it names.
    #[test]
    fn the_mode_sentence_agrees_with_its_endpoints() {
        let one: AccountConfig =
            toml::from_str("imap.server = \"imaps://a.example.org:993\"").unwrap();
        let text = one.mode().unwrap().to_string();
        assert_eq!(text, "imap syncs both ways with the local store");

        let two: AccountConfig = toml::from_str(
            r#"
            carddav.server = "https://dav.example.org/"
            caldav.server = "https://dav.example.org/"
            "#,
        )
        .unwrap();
        let text = two.mode().unwrap().to_string();
        assert_eq!(
            text,
            "caldav and carddav each sync both ways with the local store"
        );
    }

    /// Streaming is an optimisation of `retain = false`, never a mode: it
    /// needs both endpoints on a protocol that can take a body from the other.
    #[test]
    fn only_an_imap_pairing_streams_a_crossing() {
        let imap: AccountConfig = toml::from_str(
            r#"
            one-way = true
            sources.a.imap.server = "imaps://a.example.org:993"
            targets.b.imap.server = "imaps://b.example.org:993"
            "#,
        )
        .unwrap();
        assert!(imap.mode().unwrap().streams(&imap.endpoints().unwrap()));

        let dav: AccountConfig = toml::from_str(
            r#"
            one-way = true
            sources.a.carddav.server = "https://a.example.org/"
            sources.a.carddav.auth.bearer.token.raw = "tok"
            targets.b.carddav.server = "https://b.example.org/"
            targets.b.carddav.auth.bearer.token.raw = "tok"
            "#,
        )
        .unwrap();
        assert!(
            !dav.mode().unwrap().streams(&dav.endpoints().unwrap()),
            "a DAV crossing is staged and released, which `retain` cannot tell apart",
        );
    }
}
