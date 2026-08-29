//! # Flag
//!
//! An email flag (keyword) shared across all protocols: its wire spelling
//! as observed on the backend, plus an [`IanaFlag`] classification when
//! that spelling matches a registered keyword.
//!
//! Equality, ordering and hashing are IANA-first, so `\Seen`, `$seen` and
//! `seen` collapse to one logical flag while custom keywords compare
//! case-insensitively. That is what makes a `BTreeSet<Flag>` a normalised
//! set across backends.

use std::{cmp::Ordering, hash::Hash};

use serde::{Deserialize, Serialize};

/// A flag attached to an item, keeping its wire spelling and IANA tag.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Flag {
    raw: String,
    iana: Option<IanaFlag>,
}

/// IANA-registered email keywords, per the canonical table at
/// <https://www.iana.org/assignments/imap-jmap-keywords/>.
///
/// Declaration order is the derived [`Ord`], which is what gives stable
/// per-keyword sorting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IanaFlag {
    Seen,
    Answered,
    Flagged,
    Draft,
    /// A sync verb rather than a flag to propagate.
    ///
    /// Adapters round-trip `\Deleted` so single-side workflows behave,
    /// but a sync translates it into a delete on the other side.
    Deleted,
    Forwarded,
    Junk,
    NotJunk,
    Phishing,
    Important,
    MdnSent,
}

impl Flag {
    /// Builds a [`Flag`] from a wire spelling kept verbatim, deriving its
    /// IANA classification with [`classify_iana`].
    pub fn from_raw(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        let iana = classify_iana(&raw);
        Self { raw, iana }
    }

    /// Builds a [`Flag`] from an [`IanaFlag`] and its canonical wire
    /// spelling (`\Seen`, `$Forwarded`, …).
    #[allow(dead_code)]
    pub fn from_iana(iana: IanaFlag) -> Self {
        Self {
            raw: canonical_raw(iana).to_string(),
            iana: Some(iana),
        }
    }

    /// The wire spelling as observed on the backend.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The IANA classification, `None` for a custom keyword.
    #[cfg_attr(not(any(feature = "imap", feature = "msgraph")), allow(dead_code))]
    pub fn iana(&self) -> Option<IanaFlag> {
        self.iana
    }
}

impl PartialEq for Flag {
    fn eq(&self, other: &Self) -> bool {
        match (self.iana, other.iana) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self.raw.eq_ignore_ascii_case(&other.raw),
            _ => false,
        }
    }
}

impl Eq for Flag {}

impl Ord for Flag {
    /// IANA-tagged flags sort before custom keywords.
    ///
    /// Custom keywords compare on their lowercase raw text, so ordering
    /// agrees with equality.
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.iana, other.iana) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => self
                .raw
                .to_ascii_lowercase()
                .cmp(&other.raw.to_ascii_lowercase()),
        }
    }
}

impl PartialOrd for Flag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for Flag {
    /// Hashes the IANA tag, or the lowercase raw bytes when there is
    /// none, so that values comparing equal hash equal.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self.iana {
            Some(iana) => {
                0u8.hash(state);
                iana.hash(state);
            }
            None => {
                1u8.hash(state);
                for b in self.raw.as_bytes() {
                    b.to_ascii_lowercase().hash(state);
                }
            }
        }
    }
}

/// Classifies a wire spelling (one leading `\` or `$` stripped, case
/// insensitive) against the IANA table; `None` for a custom keyword.
pub fn classify_iana(raw: &str) -> Option<IanaFlag> {
    let stripped = raw
        .strip_prefix('\\')
        .or_else(|| raw.strip_prefix('$'))
        .unwrap_or(raw);

    match stripped.to_ascii_lowercase().as_str() {
        "seen" => Some(IanaFlag::Seen),
        "answered" => Some(IanaFlag::Answered),
        "flagged" => Some(IanaFlag::Flagged),
        "draft" => Some(IanaFlag::Draft),
        "deleted" => Some(IanaFlag::Deleted),
        "forwarded" => Some(IanaFlag::Forwarded),
        "junk" => Some(IanaFlag::Junk),
        "notjunk" => Some(IanaFlag::NotJunk),
        "phishing" => Some(IanaFlag::Phishing),
        "important" => Some(IanaFlag::Important),
        "mdnsent" => Some(IanaFlag::MdnSent),
        _ => None,
    }
}

/// Canonical wire spelling of an IANA keyword.
///
/// The four RFC 3501 system flags take the `\Capital` form, the rest the
/// `$Capital` form of the IANA mail keywords registry.
fn canonical_raw(iana: IanaFlag) -> &'static str {
    match iana {
        IanaFlag::Seen => "\\Seen",
        IanaFlag::Answered => "\\Answered",
        IanaFlag::Flagged => "\\Flagged",
        IanaFlag::Draft => "\\Draft",
        IanaFlag::Deleted => "\\Deleted",
        IanaFlag::Forwarded => "$Forwarded",
        IanaFlag::Junk => "$Junk",
        IanaFlag::NotJunk => "$NotJunk",
        IanaFlag::Phishing => "$Phishing",
        IanaFlag::Important => "$Important",
        IanaFlag::MdnSent => "$MDNSent",
    }
}

/// Direction of a flag store operation.
///
/// `Add` and `Remove` patch the existing set, `Set` replaces it. Every
/// backend implements all three, but the sync engine emits only `Set`,
/// pushing a whole reconciled set at once.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub enum FlagOp {
    Add,
    Remove,
    Set,
}

#[cfg(all(test, feature = "imap"))]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn classify_strips_prefix_and_is_case_insensitive() {
        assert_eq!(classify_iana("\\Seen"), Some(IanaFlag::Seen));
        assert_eq!(classify_iana("$seen"), Some(IanaFlag::Seen));
        assert_eq!(classify_iana("SEEN"), Some(IanaFlag::Seen));
        assert_eq!(classify_iana("$MDNSent"), Some(IanaFlag::MdnSent));
        assert_eq!(classify_iana("foo"), None);
    }

    #[test]
    fn from_raw_populates_iana_when_recognised() {
        let f = Flag::from_raw("\\Seen");
        assert_eq!(f.raw(), "\\Seen");
        assert_eq!(f.iana(), Some(IanaFlag::Seen));

        let f = Flag::from_raw("custom-label");
        assert_eq!(f.raw(), "custom-label");
        assert_eq!(f.iana(), None);
    }

    #[test]
    fn iana_uses_canonical_spelling() {
        assert_eq!(Flag::from_iana(IanaFlag::Seen).raw(), "\\Seen");
        assert_eq!(Flag::from_iana(IanaFlag::Forwarded).raw(), "$Forwarded");
        assert_eq!(Flag::from_iana(IanaFlag::MdnSent).raw(), "$MDNSent");
    }

    #[test]
    fn equality_collapses_wire_variants() {
        assert_eq!(Flag::from_raw("\\Seen"), Flag::from_raw("$seen"));
        assert_eq!(Flag::from_raw("\\Seen"), Flag::from_iana(IanaFlag::Seen));
        assert_eq!(Flag::from_raw("FOO"), Flag::from_raw("foo"));
        assert_ne!(Flag::from_raw("foo"), Flag::from_iana(IanaFlag::Seen));
        assert_ne!(Flag::from_raw("foo"), Flag::from_raw("bar"));
    }

    #[test]
    fn btreeset_dedupes_across_spellings() {
        let mut set: BTreeSet<Flag> = BTreeSet::new();
        set.insert(Flag::from_raw("\\Seen"));
        set.insert(Flag::from_raw("$seen"));
        set.insert(Flag::from_iana(IanaFlag::Seen));
        set.insert(Flag::from_raw("custom"));
        set.insert(Flag::from_raw("CUSTOM"));
        assert_eq!(set.len(), 2);
    }
}
