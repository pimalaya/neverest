//! # Store sidecar state
//!
//! The two things the store cannot answer about the previous run: which
//! collection-id layout it was written with, and what mode it ran under.
//!
//! A store keyed on bare collection names would read back as a set of empty
//! collections instead of being refused, and a run that would discard what the
//! previous mode kept is refused before it opens anything.
//!
//! It lives in neverest.json beside pimdir.db, deliberately outside the store:
//! it is this crate's bookkeeping, not part of the pimdir format.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use log::warn;
use serde::{Deserialize, Serialize};

use crate::config::AccountMode;

/// The current collection-id layout: `<namespace>/<name>`, kind on the row.
///
/// Layout 0 is the unnamespaced ancestor, which is not read.
const LAYOUT: u32 = 1;

/// The sidecar file name, beside `pimdir.db` in the store directory.
const FILE: &str = "neverest.json";

/// The account mode a store was last synced under.
///
/// Only what a comparison needs: the endpoint counts rather than their names,
/// renaming already orphaning the bindings, plus the two flags that decide
/// whether a run discards anything.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ModeStamp {
    /// How many sources the account declared.
    #[serde(default)]
    pub sources: usize,
    /// How many targets it declared, zero being the offline replica.
    #[serde(default)]
    pub targets: usize,
    /// Whether the sources overwrote the other side.
    #[serde(default)]
    pub one_way: bool,
    /// Whether the store kept bodies.
    #[serde(default)]
    pub retain: bool,
}

impl From<&AccountMode> for ModeStamp {
    fn from(mode: &AccountMode) -> Self {
        Self {
            sources: mode.sources.len(),
            targets: mode.targets.len(),
            one_way: mode.one_way,
            retain: mode.retain,
        }
    }
}

/// The sidecar neverest keeps beside a store, naming what wrote it.
///
/// It is what tells a store this build can read from one an earlier draft of
/// the format wrote, and it carries the mode every run is compared against.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct StoreState {
    /// The collection-id layout the store was written with.
    pub layout: u32,
    /// The mode the last run synced under, absent before modes were stamped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ModeStamp>,
    /// Set once the user accepted the mode, so the refusal stops coming back.
    #[serde(default, skip_serializing_if = "is_false")]
    pub mode_accepted: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl StoreState {
    /// Reads the sidecar, refusing a store this version cannot read.
    ///
    /// A store directory holding a pimdir.db but no sidecar predates namespaced
    /// collection ids: every collection would be looked up under a key nothing
    /// wrote to, and the sync would report a healthy run over an empty replica.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = Self::path(dir);

        if !path.exists() {
            if dir.join("pimdir.db").exists() {
                bail!(
                    "The store at {} was written before collection ids carried their \
                     namespace, and is not read. Drop it with `neverest sync --reset` and let it \
                     resync.",
                    dir.display(),
                );
            }

            return Ok(Self {
                layout: LAYOUT,
                ..Default::default()
            });
        }

        let raw =
            fs::read_to_string(&path).with_context(|| format!("Read {} error", path.display()))?;
        let state: Self = serde_json::from_str(&raw)
            .with_context(|| format!("Parse {} error", path.display()))?;

        if state.layout != LAYOUT {
            bail!(
                "The store at {} uses collection-id layout {} and this neverest writes {LAYOUT}; \
                 drop it with `neverest sync --reset` and let it resync.",
                dir.display(),
                state.layout,
            );
        }

        Ok(state)
    }

    /// Stamps a store this version just created, clearing any previous record.
    ///
    /// Whoever materializes pimdir.db owes the sidecar: a store created without
    /// this stamp reads as the unnamespaced ancestor, so it is refused on the
    /// next run and again after the `--reset` the refusal asks for.
    pub fn stamp(dir: &Path, mode: Option<&AccountMode>) -> Result<()> {
        Self {
            layout: LAYOUT,
            mode: mode.map(ModeStamp::from),
            mode_accepted: false,
        }
        .save(dir)
    }

    /// Writes the sidecar back, stamping the current layout.
    pub fn save(&mut self, dir: &Path) -> Result<()> {
        self.layout = LAYOUT;

        let path = Self::path(dir);
        let raw = serde_json::to_string_pretty(self).context("Serialize store state error")?;

        fs::write(&path, raw).with_context(|| format!("Write {} error", path.display()))
    }

    /// Refuses a run whose mode would discard what the previous one kept.
    ///
    /// Only turning `one-way` on destroys anything, and the first run does it,
    /// so there is no second chance to ask; softer changes only warn. Other
    /// configuration is not compared: forcing a resync would cost a mailbox.
    pub fn check_mode(&self, mode: &AccountMode) -> Result<()> {
        let Some(previous) = self.mode else {
            return Ok(());
        };
        let current = ModeStamp::from(mode);

        if previous == current {
            return Ok(());
        }

        if !previous.one_way && current.one_way && !self.mode_accepted {
            bail!(
                "This account synced both ways until now, and `one-way = true` makes the \
                 sources authoritative: the next run discards whatever the other side changed \
                 on its own, rather than merging it. Re-run with `--accept-mode` once you are \
                 sure, and neverest will remember."
            );
        }

        if previous.retain && !current.retain {
            warn!(
                "the store no longer keeps bodies; the ones already stored stay, unreferenced, \
                 until `pimdir gc`"
            );
        }

        if previous.sources != current.sources || previous.targets != current.targets {
            warn!(
                "endpoints changed, {} source(s) and {} target(s) where the last run had {} and {}",
                current.sources, current.targets, previous.sources, previous.targets,
            );
        }

        Ok(())
    }

    /// Remembers the mode this run synced under.
    ///
    /// `accepted` records that the user answered a refusal, so it stops coming
    /// back.
    pub fn record_mode(&mut self, mode: &AccountMode, accepted: bool) {
        self.mode = Some(ModeStamp::from(mode));
        self.mode_accepted = accepted;
    }

    fn path(dir: &Path) -> PathBuf {
        dir.join(FILE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(sources: usize, targets: usize, one_way: bool, retain: bool) -> AccountMode {
        AccountMode {
            sources: (0..sources).map(|i| format!("s{i}")).collect(),
            targets: (0..targets).map(|i| format!("t{i}")).collect(),
            one_way,
            retain,
        }
    }

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
        StoreState::stamp(dir.path(), None).unwrap();

        let state = StoreState::load(dir.path()).unwrap();
        assert_eq!(state.layout, LAYOUT);
    }

    #[test]
    fn an_empty_directory_starts_at_the_current_layout() {
        let dir = tempfile::tempdir().unwrap();
        let state = StoreState::load(dir.path()).unwrap();
        assert_eq!(state.layout, LAYOUT);
        assert!(state.mode.is_none());
    }

    /// The one transition that destroys data, and the only one that blocks.
    #[test]
    fn turning_one_way_on_refuses_the_run_that_would_discard() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = StoreState::load(dir.path()).unwrap();
        state.record_mode(&mode(1, 1, false, false), false);
        state.save(dir.path()).unwrap();

        let state = StoreState::load(dir.path()).unwrap();
        let err = state
            .check_mode(&mode(1, 1, true, false))
            .unwrap_err()
            .to_string();
        assert!(err.contains("--accept-mode"), "got {err}");
    }

    /// Accepting is remembered, so an answered refusal does not come back.
    #[test]
    fn an_accepted_mode_stops_refusing() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = StoreState::load(dir.path()).unwrap();
        state.record_mode(&mode(1, 1, false, false), false);
        state.record_mode(&mode(1, 1, true, false), true);
        state.save(dir.path()).unwrap();

        let state = StoreState::load(dir.path()).unwrap();
        state.check_mode(&mode(1, 1, true, false)).unwrap();
    }

    /// Turning one-way off merges where it overwrote: nothing lost, no gate.
    #[test]
    fn turning_one_way_off_is_not_gated() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = StoreState::load(dir.path()).unwrap();
        state.record_mode(&mode(1, 1, true, false), false);
        state.save(dir.path()).unwrap();

        let state = StoreState::load(dir.path()).unwrap();
        state.check_mode(&mode(1, 1, false, false)).unwrap();
    }

    /// A store with no recorded mode has nothing to compare against.
    ///
    /// That is the first run after an upgrade as well as the first run ever.
    #[test]
    fn an_unstamped_store_is_not_gated() {
        let dir = tempfile::tempdir().unwrap();
        let state = StoreState::load(dir.path()).unwrap();
        state.check_mode(&mode(1, 1, true, false)).unwrap();
    }

    /// Dropping `retain` and adding an endpoint report without blocking.
    ///
    /// The first leaves every stored body in place, the second writes where the
    /// account was already writing.
    #[test]
    fn a_softer_change_reports_without_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = StoreState::load(dir.path()).unwrap();
        state.record_mode(&mode(1, 0, false, true), false);
        state.save(dir.path()).unwrap();

        let state = StoreState::load(dir.path()).unwrap();
        state.check_mode(&mode(2, 0, false, false)).unwrap();
    }
}
