//! Neverest's own state beside the pimdir store.
//!
//! Two things the store cannot answer, both about the *previous* run:
//!
//! - which collection-id layout the store was written with, so a store keyed on
//!   bare collection names is refused loudly instead of reading back as a set of
//!   empty collections;
//! - what each namespace derived last time, so a run whose derivation moved can
//!   name the change rather than leave it to be noticed.
//!
//! It lives in `neverest.json` beside `pimdir.db`, deliberately outside the
//! store: it is this crate's bookkeeping, not part of the pimdir format, and a
//! store shared with another reader owes it nothing.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// The current collection-id layout: `<namespace>/<name>`, with the kind on the
/// collection row. Layout 0 is the unnamespaced ancestor, which is not read.
const LAYOUT: u32 = 1;

/// The sidecar file name, beside `pimdir.db` in the store directory.
const FILE: &str = "neverest.json";

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct StoreState {
    /// The collection-id layout the store was written with.
    pub layout: u32,
    /// What each namespace derived on the last run, keyed `<media-type>/<namespace>`.
    #[serde(default)]
    pub bodies: BTreeMap<String, String>,
}

impl StoreState {
    /// Reads the sidecar, refusing a store this version cannot read.
    ///
    /// A store directory holding a `pimdir.db` but no sidecar predates
    /// namespaced collection ids. Every collection would be looked up under a
    /// key nothing was ever written to, so the sync would report a healthy run
    /// over an empty replica. That silence is the whole reason this file
    /// exists.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = Self::path(dir);

        if !path.exists() {
            if dir.join("pimdir.db").exists() {
                bail!(
                    "The store at `{}` was written before collection ids carried their \
                     namespace, and is not read. Drop it with `neverest sync --reset` and let it \
                     resync: the store is a derived cache, so it costs a resync and loses only \
                     un-pushed local mutation.",
                    dir.display(),
                );
            }

            return Ok(Self {
                layout: LAYOUT,
                bodies: BTreeMap::new(),
            });
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("Read `{}` error", path.display()))?;
        let state: Self = serde_json::from_str(&raw)
            .with_context(|| format!("Parse `{}` error", path.display()))?;

        if state.layout != LAYOUT {
            bail!(
                "The store at `{}` uses collection-id layout {} and this neverest writes {LAYOUT}; \
                 drop it with `neverest sync --reset` and let it resync.",
                dir.display(),
                state.layout,
            );
        }

        Ok(state)
    }

    /// Stamps a store this version has just created, and clears whatever a
    /// previous store in the same directory derived.
    ///
    /// Whoever materializes `pimdir.db` owes the sidecar: [`StoreState::load`]
    /// reads a store directory holding one without the other as the
    /// unnamespaced ancestor, so a store created without this stamp is refused
    /// on the next run, and refused again after the `--reset` the refusal asks
    /// for, since resetting recreates the store the same way.
    pub fn stamp(dir: &Path) -> Result<()> {
        Self::default().save(dir)
    }

    /// Writes the sidecar back, stamping the current layout.
    pub fn save(&mut self, dir: &Path) -> Result<()> {
        self.layout = LAYOUT;

        let path = Self::path(dir);
        let raw = serde_json::to_string_pretty(self).context("Serialize store state error")?;

        fs::write(&path, raw).with_context(|| format!("Write `{}` error", path.display()))
    }

    /// What the previous run derived for a namespace, when it differed from
    /// `bodies`. Recording the new value is the caller's job, through
    /// [`StoreState::record`].
    pub fn previous(&self, key: &str, bodies: &str) -> Option<String> {
        self.bodies
            .get(key)
            .filter(|previous| previous.as_str() != bodies)
            .cloned()
    }

    /// Remembers what a namespace derived this run.
    pub fn record(&mut self, key: String, bodies: String) {
        self.bodies.insert(key, bodies);
    }

    fn path(dir: &Path) -> PathBuf {
        dir.join(FILE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_store_written_before_namespaced_ids_is_refused_with_its_remedy() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pimdir.db"), b"").unwrap();

        let err = StoreState::load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("--reset"), "got {err}");
    }

    #[test]
    fn a_store_this_version_created_is_not_taken_for_the_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pimdir.db"), b"").unwrap();
        StoreState::stamp(dir.path()).unwrap();

        let state = StoreState::load(dir.path()).unwrap();
        assert_eq!(state.layout, LAYOUT);
    }

    #[test]
    fn stamping_forgets_what_the_previous_store_derived() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = StoreState::default();
        state.record(String::from("message/rfc822/mail"), String::from("all"));
        state.save(dir.path()).unwrap();

        StoreState::stamp(dir.path()).unwrap();

        let state = StoreState::load(dir.path()).unwrap();
        assert_eq!(state.previous("message/rfc822/mail", "none"), None);
    }

    #[test]
    fn an_empty_directory_starts_at_the_current_layout() {
        let dir = tempfile::tempdir().unwrap();
        let state = StoreState::load(dir.path()).unwrap();
        assert_eq!(state.layout, LAYOUT);
        assert!(state.bodies.is_empty());
    }

    #[test]
    fn a_derivation_that_moved_is_named_and_one_that_held_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = StoreState::load(dir.path()).unwrap();

        assert_eq!(state.previous("message/rfc822/mail", "all"), None);

        state.record(String::from("message/rfc822/mail"), String::from("all"));
        state.save(dir.path()).unwrap();

        let state = StoreState::load(dir.path()).unwrap();
        assert_eq!(state.previous("message/rfc822/mail", "all"), None);
        assert_eq!(
            state.previous("message/rfc822/mail", "none").as_deref(),
            Some("all"),
        );
    }
}
