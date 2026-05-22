//! Per-account post-sync snapshot.
//!
//! The cache stores, per side, per mailbox, the set of envelopes the
//! engine last saw — keyed by content hash so a copy on one side
//! resolves to the same entry as the original on the other side. The
//! diff at the next run combines the two live envelope lists with
//! this snapshot to tell genuine additions/deletions from no-op
//! re-listings.
//!
//! Format is JSON for simplicity; sqlite is overkill for the
//! per-account scale neverest targets and would pull a build-time C
//! dependency. Escalate only when a real perf problem shows up.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use io_email::flag::Flag;
use serde::{Deserialize, Serialize};

use crate::side::Side;

/// Resolves `<cache_dir>/neverest/<account>/state.json`. Bails when
/// the OS reports no cache dir (very rare; usually means a missing
/// `HOME` or `XDG_CACHE_HOME`).
pub fn cache_path(account: &str) -> Result<PathBuf> {
    let base = dirs::cache_dir().context("Cannot resolve XDG cache directory")?;
    Ok(base.join("neverest").join(account).join("state.json"))
}

/// Full snapshot serialized as JSON. Loaded once at sync start, saved
/// once at sync end. Live envelope lists are NOT included — only the
/// minimum needed to detect changes against the next live list.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CacheSnapshot {
    /// `(side → mailbox → content-key → flag set)`. The id stored
    /// alongside is the backend-native id observed on that side; it
    /// lets the next-run diff issue flag updates without re-listing
    /// the mailbox just to recover the id.
    pub sides: HashMap<Side, MailboxSnapshots>,
}

pub type MailboxSnapshots = HashMap<String, EnvelopeSnapshots>;

/// Map keyed by the content hash from [`crate::sync::key::envelope_key`]
/// (serialized as a string because JSON object keys must be strings).
pub type EnvelopeSnapshots = HashMap<String, EnvelopeEntry>;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnvelopeEntry {
    pub id: String,
    #[serde(default)]
    pub flags: BTreeSet<Flag>,
}

impl CacheSnapshot {
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .with_context(|| format!("Parse cache `{}` error", path.display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => {
                bail!("Read cache `{}` error: {err}", path.display());
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Create cache dir `{}` error", parent.display()))?;
        }
        let bytes = serde_json::to_vec_pretty(self).context("Serialize cache snapshot error")?;
        fs::write(path, bytes)
            .with_context(|| format!("Write cache `{}` error", path.display()))?;
        Ok(())
    }

    pub fn envelopes(&self, side: Side, mailbox: &str) -> Option<&EnvelopeSnapshots> {
        self.sides.get(&side)?.get(mailbox)
    }

    pub fn set_envelopes(&mut self, side: Side, mailbox: String, entries: EnvelopeSnapshots) {
        self.sides.entry(side).or_default().insert(mailbox, entries);
    }
}
