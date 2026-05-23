//! Per-account post-sync snapshot.
//!
//! The cache stores, per side, per mailbox, the set of messages the
//! engine last saw, keyed by content hash so a copy on one side
//! resolves to the same entry as the original on the other side. The
//! diff at the next run combines the two live message lists with this
//! snapshot to tell genuine additions/deletions from no-op re-listings.
//!
//! Format is JSON for simplicity; sqlite is overkill for the
//! per-account scale neverest targets and would pull a build-time C
//! dependency. Escalate only when a real perf problem shows up.

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use io_email::flag::Flag;
use serde::{Deserialize, Serialize};

use crate::{
    side::Side,
    sync::{hunk::MailboxHunk, report::PatchEntry},
};

/// Resolves `<cache_dir>/neverest/<account>/state.json`. Bails when
/// the OS reports no cache dir (very rare; usually means a missing
/// `HOME` or `XDG_CACHE_HOME`).
pub fn cache_path(account: &str) -> Result<PathBuf> {
    let base = dirs::cache_dir().context("Cannot resolve XDG cache directory")?;
    Ok(base.join("neverest").join(account).join("state.json"))
}

/// Full snapshot serialized as JSON. Loaded once at sync start, saved
/// once at sync end. Live message lists are NOT included; only the
/// minimum needed to detect changes against the next live list, plus
/// the opaque per-`(side, mailbox)` state blob the LCD dispatcher
/// produced on the last call.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CacheSnapshot {
    /// `(side → mailbox → content-key → flag set)`. The id stored
    /// alongside is the backend-native id observed on that side; it
    /// lets the next-run diff issue flag updates without re-listing
    /// the mailbox just to recover the id.
    #[serde(default)]
    pub sides: HashMap<Side, MailboxSnapshots>,

    /// Opaque per-`(side, mailbox)` checkpoint produced by
    /// [`io_email::client::EmailClientStd::diff_envelopes`].
    /// Bytes are private to the backend impl (IMAP packs
    /// `(uid_validity, highest_mod_seq, highest_uid)`; JMAP stores
    /// the raw `Email/state` string bytes); neverest treats them as
    /// `Vec<u8>` and persists them verbatim. JMAP's account-global
    /// state is redundantly stored per-mailbox for uniform shape;
    /// storage cost is negligible.
    #[serde(default, with = "states_serde")]
    pub states: HashMap<Side, HashMap<String, Vec<u8>>>,

    /// Opaque per-`side` checkpoint produced by
    /// [`io_email::client::EmailClientStd::diff_mailboxes`]; JMAP
    /// stores `Mailbox/state` bytes. Backends without an
    /// account-global mailbox-set token leave this absent.
    #[serde(default, with = "mailbox_states_serde")]
    pub mailbox_states: HashMap<Side, Vec<u8>>,
}

pub type MailboxSnapshots = HashMap<String, MessageSnapshots>;

/// Map keyed by the content hash from [`crate::sync::key::message_key`]
/// (serialized as a string because JSON object keys must be strings).
pub type MessageSnapshots = HashMap<String, MessageEntry>;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MessageEntry {
    pub id: String,
    #[serde(default)]
    pub flags: BTreeSet<Flag>,
}

/// Helper module so `Vec<u8>` state blobs round-trip through JSON as
/// base64 strings instead of arrays-of-numbers. Keeps the cache file
/// human-readable enough for `jq` / `cat`.
mod states_serde {
    use std::collections::HashMap;

    use base64::{Engine, prelude::BASE64_STANDARD};
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

    use crate::side::Side;

    pub fn serialize<S: Serializer>(
        states: &HashMap<Side, HashMap<String, Vec<u8>>>,
        ser: S,
    ) -> Result<S::Ok, S::Error> {
        let encoded: HashMap<&Side, HashMap<&String, String>> = states
            .iter()
            .map(|(side, mailbox_states)| {
                let inner: HashMap<&String, String> = mailbox_states
                    .iter()
                    .map(|(mailbox, bytes)| (mailbox, BASE64_STANDARD.encode(bytes)))
                    .collect();
                (side, inner)
            })
            .collect();
        encoded.serialize(ser)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        de: D,
    ) -> Result<HashMap<Side, HashMap<String, Vec<u8>>>, D::Error> {
        let encoded: HashMap<Side, HashMap<String, String>> = HashMap::deserialize(de)?;
        encoded
            .into_iter()
            .map(|(side, mailbox_states)| {
                let inner: Result<HashMap<String, Vec<u8>>, _> = mailbox_states
                    .into_iter()
                    .map(|(mailbox, encoded)| {
                        BASE64_STANDARD
                            .decode(&encoded)
                            .map(|bytes| (mailbox, bytes))
                            .map_err(D::Error::custom)
                    })
                    .collect();
                inner.map(|m| (side, m))
            })
            .collect()
    }
}

/// Sister adapter for the per-side mailbox-set checkpoint
/// ([`CacheSnapshot::mailbox_states`]); same base64 trick at one less
/// level of nesting.
mod mailbox_states_serde {
    use std::collections::HashMap;

    use base64::{Engine, prelude::BASE64_STANDARD};
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

    use crate::side::Side;

    pub fn serialize<S: Serializer>(
        states: &HashMap<Side, Vec<u8>>,
        ser: S,
    ) -> Result<S::Ok, S::Error> {
        let encoded: HashMap<&Side, String> = states
            .iter()
            .map(|(side, bytes)| (side, BASE64_STANDARD.encode(bytes)))
            .collect();
        encoded.serialize(ser)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        de: D,
    ) -> Result<HashMap<Side, Vec<u8>>, D::Error> {
        let encoded: HashMap<Side, String> = HashMap::deserialize(de)?;
        encoded
            .into_iter()
            .map(|(side, encoded)| {
                BASE64_STANDARD
                    .decode(&encoded)
                    .map(|bytes| (side, bytes))
                    .map_err(D::Error::custom)
            })
            .collect()
    }
}

impl CacheSnapshot {
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .context(format!("Parse cache `{}` error", path.display())),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => {
                bail!("Read cache `{}` error: {err}", path.display());
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .context(format!("Create cache dir `{}` error", parent.display()))?;
        }
        let bytes = serde_json::to_vec_pretty(self).context("Serialize cache snapshot error")?;
        fs::write(path, bytes).context(format!("Write cache `{}` error", path.display()))?;
        Ok(())
    }

    pub fn messages(&self, side: Side, mailbox: &str) -> Option<&MessageSnapshots> {
        self.sides.get(&side)?.get(mailbox)
    }

    pub fn set_messages(&mut self, side: Side, mailbox: String, entries: MessageSnapshots) {
        self.sides.entry(side).or_default().insert(mailbox, entries);
    }

    /// Borrows the per-`(side, mailbox)` [`MessageSnapshots`] mutably,
    /// creating it on demand. The engine uses this to mutate the
    /// baseline written by [`Self::set_messages`] post-apply: each
    /// successful hunk inserts / removes / updates the entry at its
    /// `content_key`, so the next sync's diff sees the just-applied
    /// state without a re-list / re-parse pass.
    pub fn messages_mut(&mut self, side: Side, mailbox: &str) -> &mut MessageSnapshots {
        self.sides
            .entry(side)
            .or_default()
            .entry(mailbox.to_string())
            .or_default()
    }

    /// Snapshot's last-known mailbox set on `side`. Used by the
    /// mailbox-level diff to tell "added on the other side" from
    /// "deleted on this side" when one side has a mailbox the other
    /// lacks.
    pub fn mailbox_names(&self, side: Side) -> HashSet<String> {
        self.sides
            .get(&side)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Drops `mailbox` from both sides' message snapshots and the
    /// matching per-`(side, mailbox)` LCD checkpoint. Called after a
    /// successful mailbox-delete hunk so the next sync does not see
    /// the deleted mailbox in `prev_*` and re-classify the deletion as
    /// an add on the surviving side. Account-level mailbox-set
    /// checkpoints in `mailbox_states` are intentionally preserved:
    /// the server has already advanced its `Mailbox/state`, and the
    /// next mailbox-set probe needs the prior token to detect that
    /// advance.
    pub fn clear_mailbox(&mut self, mailbox: &str) {
        for side_map in self.sides.values_mut() {
            side_map.remove(mailbox);
        }
        for state_map in self.states.values_mut() {
            state_map.remove(mailbox);
        }
    }

    /// Returns the opaque LCD checkpoint persisted for
    /// `(side, mailbox)`, or `None` if the next sync needs to capture
    /// a baseline.
    pub fn state(&self, side: Side, mailbox: &str) -> Option<&[u8]> {
        self.states.get(&side)?.get(mailbox).map(Vec::as_slice)
    }

    pub fn set_state(&mut self, side: Side, mailbox: String, state: Vec<u8>) {
        self.states.entry(side).or_default().insert(mailbox, state);
    }

    /// Returns the opaque mailbox-set checkpoint persisted for `side`,
    /// or `None` if the next sync needs to capture a baseline.
    pub fn mailbox_state(&self, side: Side) -> Option<&[u8]> {
        self.mailbox_states.get(&side).map(Vec::as_slice)
    }

    pub fn set_mailbox_state(&mut self, side: Side, state: Vec<u8>) {
        self.mailbox_states.insert(side, state);
    }

    /// Drops every per-mailbox snapshot + checkpoint for the account,
    /// scoped optionally to the given mailbox names. Empty
    /// `mailboxes` clears everything; otherwise only the listed names
    /// are wiped. Backs the user-facing `neverest sync --resync`
    /// flag (Phase F).
    pub fn resync(&mut self, mailboxes: &[String]) {
        if mailboxes.is_empty() {
            self.sides.clear();
            self.states.clear();
            self.mailbox_states.clear();
            return;
        }
        for mailbox in mailboxes {
            self.clear_mailbox(mailbox);
        }
    }

    /// Stage 5 of the sync: drop entries for any mailbox deleted in
    /// the mailbox patch (so the next sync does not see the dropped
    /// name in `prev_*`) and persist the snapshot. Per-mailbox
    /// `set_messages` updates happen inline during the engine's
    /// per-mailbox loop; this method no longer re-lists envelopes.
    pub fn record(&mut self, mailbox_patch: &[PatchEntry<MailboxHunk>], path: &Path) -> Result<()> {
        for entry in mailbox_patch {
            if entry.error.is_some() {
                continue;
            }
            let MailboxHunk::Delete { mailbox, .. } = &entry.hunk else {
                continue;
            };
            self.clear_mailbox(mailbox);
        }

        self.save(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_with_states() -> CacheSnapshot {
        let mut s = CacheSnapshot::default();
        // tricky bytes: zero, high-bit, len not a multiple of 3.
        let bytes = vec![0x00, 0xff, 0x42, 0x80, 0x01];
        s.set_state(Side::Left, "INBOX".into(), bytes.clone());
        s.set_state(Side::Right, "Sent".into(), vec![0xfe, 0xed]);
        s.set_mailbox_state(Side::Left, vec![0xab, 0xcd, 0x00, 0x12]);
        s.set_mailbox_state(Side::Right, vec![]);
        s.set_messages(Side::Left, "INBOX".into(), MessageSnapshots::new());
        s
    }

    #[test]
    fn states_base64_round_trip() {
        let original = snapshot_with_states();
        let json = serde_json::to_vec(&original).unwrap();
        let parsed: CacheSnapshot = serde_json::from_slice(&json).unwrap();

        assert_eq!(
            parsed.state(Side::Left, "INBOX"),
            Some([0x00, 0xff, 0x42, 0x80, 0x01].as_slice()),
        );
        assert_eq!(
            parsed.state(Side::Right, "Sent"),
            Some([0xfe, 0xed].as_slice()),
        );
    }

    #[test]
    fn mailbox_states_base64_round_trip() {
        let original = snapshot_with_states();
        let json = serde_json::to_vec(&original).unwrap();
        let parsed: CacheSnapshot = serde_json::from_slice(&json).unwrap();

        assert_eq!(
            parsed.mailbox_state(Side::Left),
            Some([0xab, 0xcd, 0x00, 0x12].as_slice()),
        );
        assert_eq!(parsed.mailbox_state(Side::Right), Some([].as_slice()));
    }

    #[test]
    fn clear_mailbox_preserves_account_global_mailbox_states() {
        let mut s = snapshot_with_states();
        // also add a second mailbox under sides + states so we can
        // assert it survives.
        s.set_state(Side::Left, "Archive".into(), vec![1, 2, 3]);
        s.set_messages(Side::Left, "Archive".into(), MessageSnapshots::new());

        s.clear_mailbox("INBOX");

        assert!(s.messages(Side::Left, "INBOX").is_none());
        assert!(s.state(Side::Left, "INBOX").is_none());
        // Account-global mailbox-set token survives the per-mailbox drop.
        assert!(s.mailbox_state(Side::Left).is_some());
        // Other mailbox untouched.
        assert!(s.state(Side::Left, "Archive").is_some());
        assert!(s.messages(Side::Left, "Archive").is_some());
    }

    #[test]
    fn resync_empty_clears_everything() {
        let mut s = snapshot_with_states();
        s.resync(&[]);
        assert!(s.state(Side::Left, "INBOX").is_none());
        assert!(s.state(Side::Right, "Sent").is_none());
        assert!(s.mailbox_state(Side::Left).is_none());
        assert!(s.mailbox_state(Side::Right).is_none());
        assert!(s.messages(Side::Left, "INBOX").is_none());
    }

    #[test]
    fn resync_named_clears_only_listed_mailbox() {
        let mut s = snapshot_with_states();
        s.set_state(Side::Left, "Archive".into(), vec![1, 2, 3]);

        s.resync(&["INBOX".to_string()]);

        assert!(s.state(Side::Left, "INBOX").is_none());
        assert!(s.state(Side::Left, "Archive").is_some());
        // Account-level mailbox tokens survive (per A.5 documented
        // semantics): they're owned by the mailbox-set probe, not the
        // per-mailbox loop.
        assert!(s.mailbox_state(Side::Left).is_some());
    }

    #[test]
    fn state_set_state_round_trip() {
        let mut s = CacheSnapshot::default();
        assert_eq!(s.state(Side::Left, "INBOX"), None);
        s.set_state(Side::Left, "INBOX".into(), vec![1, 2, 3]);
        assert_eq!(s.state(Side::Left, "INBOX"), Some([1, 2, 3].as_slice()));
    }

    #[test]
    fn mailbox_state_round_trip() {
        let mut s = CacheSnapshot::default();
        assert_eq!(s.mailbox_state(Side::Left), None);
        s.set_mailbox_state(Side::Left, vec![9, 9]);
        assert_eq!(s.mailbox_state(Side::Left), Some([9, 9].as_slice()));
    }

    #[test]
    fn load_legacy_cache_without_new_fields() {
        // Old cache file that only had `sides`; the new optional
        // `states` / `mailbox_states` fields must default to empty
        // maps instead of erroring out.
        let legacy = serde_json::json!({
            "sides": {
                "left": {
                    "INBOX": {
                        "42": { "id": "1", "flags": [] }
                    }
                }
            }
        });
        let bytes = serde_json::to_vec(&legacy).unwrap();
        let parsed: CacheSnapshot = serde_json::from_slice(&bytes).unwrap();
        assert!(parsed.messages(Side::Left, "INBOX").is_some());
        assert!(parsed.state(Side::Left, "INBOX").is_none());
        assert!(parsed.mailbox_state(Side::Left).is_none());
    }
}
