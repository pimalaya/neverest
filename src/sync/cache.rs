// This file is part of Neverest, a CLI to synchronize emails.
//
// Copyright (C) 2024-2026  soywod <pimalaya.org@posteo.net>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Per-account JSON snapshot persisting, per side and mailbox, the
//! content-keyed message set and the LCD checkpoints.

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

pub type MailboxSnapshots = HashMap<String, MessageSnapshots>;

/// Map keyed by the stringified content hash from
/// [`crate::sync::key::message_key`].
pub type MessageSnapshots = HashMap<String, MessageEntry>;

/// Full snapshot loaded at sync start and saved at sync end.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CacheSnapshot {
    /// `(side → mailbox → content-key → MessageEntry)`.
    #[serde(default)]
    pub sides: HashMap<Side, MailboxSnapshots>,

    /// Opaque per-`(side, mailbox)` envelope-diff checkpoint, kept as
    /// raw bytes (IMAP QRESYNC pack, JMAP `Email/state` string).
    #[serde(default, with = "states_serde")]
    pub states: HashMap<Side, HashMap<String, Vec<u8>>>,

    /// Opaque per-`side` mailbox-set checkpoint (JMAP `Mailbox/state`);
    /// absent on backends without an account-global token.
    #[serde(default, with = "mailbox_states_serde")]
    pub mailbox_states: HashMap<Side, Vec<u8>>,
}

impl CacheSnapshot {
    /// Resolves `<cache_dir>/neverest/<account>/state.json`.
    pub fn path(account: &str) -> Result<PathBuf> {
        let base = dirs::cache_dir().context("Cannot resolve XDG cache directory")?;
        Ok(base.join("neverest").join(account).join("state.json"))
    }

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

    /// Mutably borrows the per-`(side, mailbox)` snapshot, creating it
    /// on demand.
    pub fn messages_mut(&mut self, side: Side, mailbox: &str) -> &mut MessageSnapshots {
        self.sides
            .entry(side)
            .or_default()
            .entry(mailbox.to_string())
            .or_default()
    }

    /// Last-known mailbox name set on `side`.
    pub fn mailbox_names(&self, side: Side) -> HashSet<String> {
        self.sides
            .get(&side)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Drops `mailbox` from both sides' message snapshots and the
    /// matching per-`(side, mailbox)` checkpoint; account-level
    /// `mailbox_states` are preserved.
    pub fn clear_mailbox(&mut self, mailbox: &str) {
        for side_map in self.sides.values_mut() {
            side_map.remove(mailbox);
        }
        for state_map in self.states.values_mut() {
            state_map.remove(mailbox);
        }
    }

    /// Opaque envelope-diff checkpoint for `(side, mailbox)`, or `None`
    /// if a baseline still needs to be captured.
    pub fn state(&self, side: Side, mailbox: &str) -> Option<&[u8]> {
        self.states.get(&side)?.get(mailbox).map(Vec::as_slice)
    }

    pub fn set_state(&mut self, side: Side, mailbox: String, state: Vec<u8>) {
        self.states.entry(side).or_default().insert(mailbox, state);
    }

    /// Opaque mailbox-set checkpoint for `side`, or `None` if a
    /// baseline still needs to be captured.
    pub fn mailbox_state(&self, side: Side) -> Option<&[u8]> {
        self.mailbox_states.get(&side).map(Vec::as_slice)
    }

    pub fn set_mailbox_state(&mut self, side: Side, state: Vec<u8>) {
        self.mailbox_states.insert(side, state);
    }

    /// Drops every per-mailbox snapshot + checkpoint, restricted to
    /// `mailboxes` when non-empty.
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

    /// Drops entries for mailboxes deleted in `mailbox_patch` then
    /// persists the snapshot.
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MessageEntry {
    pub id: String,
    #[serde(default)]
    pub flags: BTreeSet<Flag>,
}

/// Serde adapter encoding `Vec<u8>` state blobs as base64 strings
/// rather than arrays-of-numbers.
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

/// Sister of [`states_serde`] for the flat per-side mailbox-set token.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_with_states() -> CacheSnapshot {
        let mut s = CacheSnapshot::default();
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
        s.set_state(Side::Left, "Archive".into(), vec![1, 2, 3]);
        s.set_messages(Side::Left, "Archive".into(), MessageSnapshots::new());

        s.clear_mailbox("INBOX");

        assert!(s.messages(Side::Left, "INBOX").is_none());
        assert!(s.state(Side::Left, "INBOX").is_none());
        assert!(s.mailbox_state(Side::Left).is_some());
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
        // NOTE: legacy cache files only had `sides`; the optional
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
