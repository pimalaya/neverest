//! # Store sidecar state
//!
//! What the store cannot answer about the previous runs: which collection-id
//! layout it was written with, and, for each account it holds, what mode that
//! account ran under and what its listings stated.
//!
//! A store keyed on another layout would read back as a set of empty
//! collections instead of being refused, and a run that would discard what
//! the previous mode kept is refused before it opens anything.
//!
//! It lives in neverest.json under the store's local/, the directory pimdir
//! reserves for its owner (pimdir STORAGE §3): it is this crate's
//! bookkeeping, not part of the pimdir format.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use log::warn;
use serde::{Deserialize, Serialize};

use crate::{config::AccountMode, error::layout_error, offline::local};

/// The current layout: source ids `<account>/<source>`, collection ids
/// `<account>/<source>/<name>`, kind on the row, one sidecar entry per
/// account.
///
/// Layout 0 is the unnamespaced ancestor, layout 1 the store an earlier
/// neverest kept per account; neither is read.
const LAYOUT: u32 = 2;

/// The sidecar file name, under the store's local/.
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

/// The sidecar neverest keeps under a store's local/, naming what wrote it.
///
/// It is what tells a store this build can read from one an earlier neverest
/// laid out, and it carries, per account, the mode every run is compared
/// against. An account is initialized once it has an entry.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct StoreState {
    /// The layout the store was written with.
    pub layout: u32,
    /// What each initialized account left, by account name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub accounts: BTreeMap<String, AccountState>,
}

/// What the sidecar keeps for one account of the store.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AccountState {
    /// The mode the last run synced under, absent before a run stamped it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ModeStamp>,
    /// Set once the user accepted the mode, so the refusal stops coming back.
    #[serde(default, skip_serializing_if = "is_false")]
    pub mode_accepted: bool,
    /// The roles a source's listing does not carry, looked up when its
    /// collection set last moved ([`crate::client::Client::lookup_roles`]),
    /// by source.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub looked_up_roles: BTreeMap<String, LookedUpRoles>,
    /// The calendars a source's last listing stated take no tasks (their
    /// `supported-calendar-component-set` leaving VTODO out), as hub ids,
    /// by source: declared `calendar.component.vtodo` `none` there (pimdir
    /// STORAGE §15.6) by every declaration, the one before the drain opening
    /// no endpoint.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub taskless: BTreeMap<String, BTreeSet<String>>,
}

/// One source's looked-up roles, with the collection set they were
/// looked up against: the same set reuses them, another asks again.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LookedUpRoles {
    /// The ids the source listed, sorted.
    pub listing: Vec<String>,
    /// The role each collection states, by id.
    pub roles: BTreeMap<String, String>,
}

impl StoreState {
    /// Reads the sidecar, refusing a store this version cannot read.
    ///
    /// A store directory holding a pimdir.db but no sidecar, a sidecar
    /// beside its database, where an earlier neverest kept it, or one of an
    /// earlier layout, was written by an earlier neverest: every collection
    /// would be looked up under a key nothing wrote to, and the sync would
    /// report a healthy run over an empty replica.
    pub fn load(dir: &Path) -> Result<Self> {
        let path = Self::path(dir);

        if !path.exists() {
            if dir.join(FILE).exists() {
                return Err(earlier(dir, "a neverest.json beside its database"));
            }
            if dir.join("pimdir.db").exists() {
                return Err(earlier(dir, "a database and no local/neverest.json"));
            }

            return Ok(Self {
                layout: LAYOUT,
                ..Default::default()
            });
        }

        let raw =
            fs::read_to_string(&path).with_context(|| format!("Read {} error", path.display()))?;

        // NOTE: the layout first, an earlier sidecar being of another shape
        // than this one parses.
        #[derive(Deserialize)]
        struct Layout {
            #[serde(default)]
            layout: u32,
        }
        let layout: Layout = serde_json::from_str(&raw)
            .with_context(|| format!("Parse {} error", path.display()))?;
        if layout.layout != LAYOUT {
            let what = format!("a neverest.json of layout {}", layout.layout);
            return Err(earlier(dir, &what));
        }

        serde_json::from_str(&raw).with_context(|| format!("Parse {} error", path.display()))
    }

    /// Whether `account` has an entry, which `init` writes.
    pub fn initialized(&self, account: &str) -> bool {
        self.accounts.contains_key(account)
    }

    /// The entry of `account`, refused when `init` never wrote one.
    pub fn account(&self, account: &str) -> Result<AccountState> {
        self.accounts.get(account).cloned().ok_or_else(|| {
            anyhow!("Account {account} not initialized, run `neverest init -a {account}` first")
        })
    }

    /// Stamps `account` a fresh entry under `mode`, the store's other entries
    /// as they were.
    ///
    /// Whoever materializes pimdir.db owes the sidecar: a store created
    /// without one reads as the unnamespaced ancestor, so `init` stamps the
    /// account it initializes, and a reset the account it starts over.
    pub fn stamp(dir: &Path, account: &str, mode: &AccountMode) -> Result<()> {
        let mut state = Self::load(dir)?;
        let entry = AccountState {
            mode: Some(ModeStamp::from(mode)),
            ..Default::default()
        };
        state.accounts.insert(account.to_owned(), entry);
        state.save(dir)
    }

    /// Writes the sidecar back, stamping the current layout.
    ///
    /// Written aside, synced, then renamed over the old one, so a run killed
    /// while saving leaves the old sidecar or the new one, never a torn file
    /// the next run refuses.
    pub fn save(&mut self, dir: &Path) -> Result<()> {
        self.layout = LAYOUT;

        let path = Self::path(dir);
        let raw = serde_json::to_string_pretty(self).context("Serialize store state error")?;

        local::ensure(dir).with_context(|| format!("Create {}", local::path(dir).display()))?;
        let staged = path.with_extension("json.new");
        let mut file =
            local::create(&staged).with_context(|| format!("Create {}", staged.display()))?;
        file.write_all(raw.as_bytes())
            .and_then(|()| file.sync_all())
            .with_context(|| format!("Write {} error", staged.display()))?;
        fs::rename(&staged, &path).with_context(|| format!("Write {} error", path.display()))
    }

    fn path(dir: &Path) -> PathBuf {
        local::path(dir).join(FILE)
    }
}

impl AccountState {
    /// Refuses a run whose mode would discard what the previous one kept,
    /// unless `accepted`, the run's `--accept-mode`, or an earlier run's,
    /// says the user knows.
    ///
    /// Only turning `one-way` on destroys anything, and the first run does it,
    /// so there is no second chance to ask; softer changes only warn. Other
    /// configuration is not compared: forcing a resync would cost a mailbox.
    pub fn check_mode(&self, mode: &AccountMode, accepted: bool) -> Result<()> {
        let Some(previous) = self.mode else {
            return Ok(());
        };
        let current = ModeStamp::from(mode);

        if previous == current {
            return Ok(());
        }

        if !previous.one_way && current.one_way && !self.mode_accepted && !accepted {
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
}

/// The refusal of a store an earlier neverest laid out, holding `what`.
fn earlier(dir: &Path, what: &str) -> anyhow::Error {
    layout_error(format!(
        "The store at {} holds {what}, which an earlier neverest wrote and this one does not \
         read: sync it with the neverest that wrote it until nothing is queued, move the \
         store directory away, then run `neverest init` again (see MIGRATION.md)",
        dir.display(),
    ))
}

fn is_false(value: &bool) -> bool {
    !value
}

#[cfg(test)]
mod tests {
    use std::sync::PoisonError;

    use pimalaya_cli::error::code_of;

    use crate::{
        conflict::merger::SPAWNS,
        error::ErrorCode,
        offline::{driver::create_store, state::*, submit::InFlight},
    };

    /// A save replaces the sidecar whole, by a rename over it, rather than
    /// rewriting it in place, where a crash leaves a torn file the next run
    /// refuses: the file after a save is another one, and nothing is left
    /// beside it.
    #[test]
    #[cfg(unix)]
    fn a_save_replaces_the_sidecar_whole() {
        use std::os::unix::fs::MetadataExt;

        let dir = tempfile::tempdir().unwrap();
        StoreState::stamp(dir.path(), "a", &mode(1, 0, false, true)).unwrap();
        let before = fs::metadata(StoreState::path(dir.path())).unwrap().ino();

        let mut state = StoreState::load(dir.path()).unwrap();
        state.save(dir.path()).unwrap();

        let after = fs::metadata(StoreState::path(dir.path())).unwrap().ino();
        assert_ne!(before, after);
        assert_eq!(names(&local::path(dir.path())), [FILE]);
        StoreState::load(dir.path()).unwrap();
    }

    /// The file names `dir` holds.
    fn names(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect()
    }

    /// The sidecar is the owner's own state: it is kept under the store's
    /// local/, the one place pimdir leaves an owner in a store directory
    /// (pimdir STORAGE §3), its user's alone, and nothing of neverest's
    /// stands beside the store's files.
    #[test]
    #[cfg(unix)]
    fn the_sidecar_is_kept_under_local() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        StoreState::stamp(dir.path(), "a", &mode(1, 0, false, true)).unwrap();

        assert_eq!(names(dir.path()), ["local"]);
        let sidecar = dir.path().join("local").join("neverest.json");
        let mode = fs::metadata(&sidecar).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    /// A sidecar beside the database is where an earlier neverest kept it:
    /// the store is refused as that neverest's, never read as one holding
    /// no account.
    #[test]
    fn a_sidecar_beside_the_database_is_refused_as_an_earlier_layout() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pimdir.db"), b"").unwrap();
        fs::write(dir.path().join(FILE), r#"{"layout":2}"#).unwrap();

        let err = StoreState::load(dir.path()).unwrap_err();
        assert!(
            format!("{err:#}").contains("beside its database"),
            "{err:#}"
        );
        assert_refused_as_earlier(err);
    }

    fn mode(sources: usize, targets: usize, one_way: bool, retain: bool) -> AccountMode {
        AccountMode {
            sources: (0..sources).map(|i| format!("s{i}")).collect(),
            targets: (0..targets).map(|i| format!("t{i}")).collect(),
            one_way,
            retain,
        }
    }

    /// The refusal of an earlier store carries `store-layout` and the
    /// reinstall in the order that keeps its queued writes, never a delete.
    fn assert_refused_as_earlier(err: anyhow::Error) {
        assert_eq!(code_of::<ErrorCode>(&err), Some(ErrorCode::StoreLayout));
        let err = format!("{err:#}");
        assert!(err.contains("until nothing is queued"), "{err}");
        assert!(err.contains("move the store directory away"), "{err}");
        assert!(!err.to_lowercase().contains("delete"), "{err}");
    }

    #[test]
    fn a_database_with_no_sidecar_is_refused_as_an_earlier_layout() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pimdir.db"), b"").unwrap();

        assert_refused_as_earlier(StoreState::load(dir.path()).unwrap_err());
    }

    /// The sidecar an earlier neverest kept per account, its mode at its
    /// root, is refused rather than read as a store of no account.
    #[test]
    fn a_sidecar_of_the_previous_shape_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pimdir.db"), b"").unwrap();
        let previous =
            r#"{"layout":1,"mode":{"sources":1,"targets":0,"one_way":false,"retain":true}}"#;
        local::ensure(dir.path()).unwrap();
        fs::write(StoreState::path(dir.path()), previous).unwrap();

        assert_refused_as_earlier(StoreState::load(dir.path()).unwrap_err());
    }

    /// A store created as `init` creates it, its sidecar written with its
    /// database, reads as this layout, and an account stamped in it as
    /// initialized; a send an earlier store had in flight is cleared.
    #[test]
    fn a_created_store_is_not_taken_for_an_earlier_one() {
        let _owning = SPAWNS.read().unwrap_or_else(PoisonError::into_inner);
        let dir = tempfile::tempdir().unwrap();
        let mark = InFlight::new(dir.path());
        mark.begin(1).unwrap();

        create_store(dir.path()).unwrap();
        StoreState::stamp(dir.path(), "a", &mode(1, 0, false, true)).unwrap();

        assert_eq!(mark.left(), None);

        let state = StoreState::load(dir.path()).unwrap();
        assert_eq!(state.layout, LAYOUT);
        assert!(state.initialized("a"));
    }

    #[test]
    fn an_empty_directory_starts_at_the_current_layout() {
        let dir = tempfile::tempdir().unwrap();
        let state = StoreState::load(dir.path()).unwrap();
        assert_eq!(state.layout, LAYOUT);
        assert!(state.accounts.is_empty());
    }

    /// An account `init` never stamped is refused naming `init` for it,
    /// whatever the store holds for the others.
    #[test]
    fn an_account_with_no_entry_is_not_initialized() {
        let dir = tempfile::tempdir().unwrap();
        StoreState::stamp(dir.path(), "work", &mode(1, 0, false, true)).unwrap();

        let state = StoreState::load(dir.path()).unwrap();
        state.account("work").unwrap();
        let err = state.account("home").unwrap_err().to_string();
        assert!(err.contains("neverest init -a home"), "{err}");
    }

    /// Each account keeps its own entry: stamping one again, or recording
    /// what one ran under, leaves the other as it was.
    #[test]
    fn each_account_keeps_its_own_entry() {
        let dir = tempfile::tempdir().unwrap();
        StoreState::stamp(dir.path(), "work", &mode(1, 1, false, false)).unwrap();
        StoreState::stamp(dir.path(), "home", &mode(1, 0, false, true)).unwrap();

        let mut state = StoreState::load(dir.path()).unwrap();
        let mut home = state.account("home").unwrap();
        home.record_mode(&mode(2, 0, false, true), false);
        home.taskless
            .insert("home/caldav".into(), BTreeSet::from(["x".into()]));
        state.accounts.insert("home".into(), home);
        state.save(dir.path()).unwrap();
        StoreState::stamp(dir.path(), "work", &mode(1, 1, false, false)).unwrap();

        let state = StoreState::load(dir.path()).unwrap();
        let home = state.account("home").unwrap();
        assert_eq!(home.mode.unwrap().sources, 2);
        assert_eq!(home.taskless.len(), 1);
        let work = state.account("work").unwrap();
        assert_eq!(work.mode.unwrap().targets, 1);
        let err = work
            .check_mode(&mode(1, 1, true, false), false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--accept-mode"), "got {err}");
        home.check_mode(&mode(2, 0, false, true), false).unwrap();
    }

    /// The one transition that destroys data, and the only one that blocks.
    #[test]
    fn turning_one_way_on_refuses_the_run_that_would_discard() {
        let mut state = AccountState::default();
        state.record_mode(&mode(1, 1, false, false), false);

        let err = state
            .check_mode(&mode(1, 1, true, false), false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--accept-mode"), "got {err}");

        // NOTE: the run the refusal names, which records the answer.
        state.check_mode(&mode(1, 1, true, false), true).unwrap();
    }

    /// Accepting is remembered, so an answered refusal does not come back.
    #[test]
    fn an_accepted_mode_stops_refusing() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = StoreState::load(dir.path()).unwrap();
        let mut account = AccountState::default();
        account.record_mode(&mode(1, 1, false, false), false);
        account.record_mode(&mode(1, 1, true, false), true);
        state.accounts.insert("a".into(), account);
        state.save(dir.path()).unwrap();

        let state = StoreState::load(dir.path()).unwrap();
        let account = state.account("a").unwrap();
        account.check_mode(&mode(1, 1, true, false), false).unwrap();
    }

    /// Turning one-way off merges where it overwrote: nothing lost, no gate.
    #[test]
    fn turning_one_way_off_is_not_gated() {
        let mut state = AccountState::default();
        state.record_mode(&mode(1, 1, true, false), false);

        state.check_mode(&mode(1, 1, false, false), false).unwrap();
    }

    /// An account with no recorded mode has nothing to compare against.
    ///
    /// That is the first run after an upgrade as well as the first run ever.
    #[test]
    fn an_unstamped_account_is_not_gated() {
        AccountState::default()
            .check_mode(&mode(1, 1, true, false), false)
            .unwrap();
    }

    /// Dropping `retain` and adding an endpoint report without blocking.
    ///
    /// The first leaves every stored body in place, the second writes where the
    /// account was already writing.
    #[test]
    fn a_softer_change_reports_without_blocking() {
        let mut state = AccountState::default();
        state.record_mode(&mode(1, 0, false, true), false);

        state.check_mode(&mode(2, 0, false, false), false).unwrap();
    }
}
