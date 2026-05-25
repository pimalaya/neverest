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

//! Snapshot-driven incremental envelope delta: one listdir plus one
//! sidecar read per entry, no protocol checkpoint needed.

use std::collections::{BTreeSet, HashMap, HashSet};

use anyhow::{Context, Result};
use io_email::{
    client::EmailClientStd,
    envelope::{Envelope, EnvelopeDiff, FlagUpdate},
    flag::Flag,
    m2dir::convert::{envelope_from, open_m2dir},
};
use mail_parser::MessageParser;

use crate::sync::cache::MessageSnapshots;

/// Computes the envelope diff for an m2dir mailbox against `prev`.
pub fn diff_envelopes(
    client: &mut EmailClientStd,
    mailbox: &str,
    prev: Option<&MessageSnapshots>,
) -> Result<EnvelopeDiff> {
    let prev_by_id: HashMap<&str, &BTreeSet<Flag>> = prev
        .map(|p| p.values().map(|e| (e.id.as_str(), &e.flags)).collect())
        .unwrap_or_default();

    let m2dir_client = client
        .as_m2dir_mut()
        .context("m2dir client not registered on this side")?;
    let m2dir = open_m2dir(m2dir_client, mailbox)?;
    let entries = m2dir_client.list_entries(m2dir.clone())?;

    let parser = MessageParser::default();
    let mut current_ids: HashSet<String> = HashSet::with_capacity(entries.len());
    let mut new_envelopes: Vec<Envelope> = Vec::new();
    let mut flag_updates: Vec<FlagUpdate> = Vec::new();

    for entry in &entries {
        let id = entry.id().to_string();
        current_ids.insert(id.clone());

        let flag_lines = m2dir_client.read_flags(&m2dir, entry.id())?;
        let current_flags: BTreeSet<Flag> = flag_lines
            .iter()
            .map(|line| Flag::from_raw(line.trim()))
            .collect();

        match prev_by_id.get(id.as_str()) {
            None => {
                let (_, bytes) = m2dir_client.get(m2dir.clone(), entry.id())?;
                let parsed = parser
                    .parse_headers(&bytes)
                    .context("Parse m2dir message headers")?;
                new_envelopes.push(envelope_from(entry, &flag_lines, &parsed));
            }
            Some(prev_flags) if **prev_flags != current_flags => {
                flag_updates.push(FlagUpdate {
                    id,
                    flags: current_flags,
                });
            }
            Some(_) => {}
        }
    }

    let vanished_ids: Vec<String> = prev_by_id
        .keys()
        .filter(|id| !current_ids.contains(**id))
        .map(|id| id.to_string())
        .collect();

    Ok(EnvelopeDiff::Incremental {
        new_state: Vec::new(),
        flag_updates,
        new_envelopes,
        vanished_ids,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use io_email::flag::{Flag, IanaFlag};
    use io_m2dir::client::M2dirClient;
    use tempfile::tempdir;

    use super::*;
    use crate::sync::cache::{MessageEntry, MessageSnapshots};

    fn snapshot_with(entries: &[(&str, &[Flag])]) -> MessageSnapshots {
        let mut snap = MessageSnapshots::new();
        for (i, (id, flags)) in entries.iter().enumerate() {
            snap.insert(
                i.to_string(),
                MessageEntry {
                    id: (*id).to_string(),
                    flags: flags.iter().cloned().collect(),
                },
            );
        }
        snap
    }

    fn mk_client(root: &std::path::Path) -> EmailClientStd {
        let m2 = M2dirClient::new(root.to_string_lossy().into_owned());
        m2.init_store().unwrap();
        EmailClientStd::from(m2)
    }

    const RAW_A: &[u8] = b"Message-ID: <a@example.org>\r\n\
                           From: alice@example.org\r\n\
                           Subject: hello\r\n\
                           Date: Tue, 15 Apr 1994 08:12:31 GMT\r\n\
                           \r\nbody\r\n";

    const RAW_B: &[u8] = b"Message-ID: <b@example.org>\r\n\
                           From: bob@example.org\r\n\
                           Subject: re: hello\r\n\
                           Date: Wed, 16 Apr 1994 08:12:31 GMT\r\n\
                           \r\nbody\r\n";

    #[test]
    fn empty_mailbox_empty_prev_yields_no_changes() {
        let dir = tempdir().unwrap();
        let mut client = mk_client(dir.path());
        client.create_mailbox("inbox").unwrap();

        let diff = diff_envelopes(&mut client, "inbox", None).unwrap();
        match diff {
            EnvelopeDiff::Incremental {
                new_envelopes,
                flag_updates,
                vanished_ids,
                ..
            } => {
                assert!(new_envelopes.is_empty());
                assert!(flag_updates.is_empty());
                assert!(vanished_ids.is_empty());
            }
            other => panic!("expected Incremental, got {other:?}"),
        }
    }

    #[test]
    fn first_sync_surfaces_all_entries_as_new() {
        let dir = tempdir().unwrap();
        let mut client = mk_client(dir.path());
        client.create_mailbox("inbox").unwrap();
        let _id_a = client.add_message("inbox", &[], RAW_A.to_vec()).unwrap();
        let _id_b = client.add_message("inbox", &[], RAW_B.to_vec()).unwrap();

        let diff = diff_envelopes(&mut client, "inbox", None).unwrap();
        let EnvelopeDiff::Incremental { new_envelopes, .. } = diff else {
            panic!("expected Incremental");
        };
        assert_eq!(new_envelopes.len(), 2);
        let mids: BTreeSet<_> = new_envelopes
            .iter()
            .filter_map(|e| e.message_id.clone())
            .collect();
        assert!(mids.contains("a@example.org"));
        assert!(mids.contains("b@example.org"));
    }

    #[test]
    fn unchanged_mailbox_yields_empty_diff() {
        let dir = tempdir().unwrap();
        let mut client = mk_client(dir.path());
        client.create_mailbox("inbox").unwrap();
        let id_a = client.add_message("inbox", &[], RAW_A.to_vec()).unwrap();
        let id_b = client.add_message("inbox", &[], RAW_B.to_vec()).unwrap();

        let prev = snapshot_with(&[(&id_a, &[]), (&id_b, &[])]);
        let diff = diff_envelopes(&mut client, "inbox", Some(&prev)).unwrap();
        let EnvelopeDiff::Incremental {
            new_envelopes,
            flag_updates,
            vanished_ids,
            ..
        } = diff
        else {
            panic!("expected Incremental");
        };
        assert!(new_envelopes.is_empty(), "expected no new envelopes");
        assert!(flag_updates.is_empty(), "expected no flag updates");
        assert!(vanished_ids.is_empty(), "expected no vanished ids");
    }

    #[test]
    fn flag_change_surfaces_as_update_no_parse() {
        let dir = tempdir().unwrap();
        let mut client = mk_client(dir.path());
        client.create_mailbox("inbox").unwrap();
        let id_a = client.add_message("inbox", &[], RAW_A.to_vec()).unwrap();

        let prev = snapshot_with(&[(&id_a, &[])]);

        client
            .add_flags(
                "inbox",
                &[id_a.as_str()],
                &[Flag::from_iana(IanaFlag::Seen)],
            )
            .unwrap();

        let diff = diff_envelopes(&mut client, "inbox", Some(&prev)).unwrap();
        let EnvelopeDiff::Incremental {
            new_envelopes,
            flag_updates,
            vanished_ids,
            ..
        } = diff
        else {
            panic!("expected Incremental");
        };
        assert!(new_envelopes.is_empty());
        assert_eq!(flag_updates.len(), 1);
        assert_eq!(flag_updates[0].id, id_a);
        assert!(flag_updates[0].flags.iter().any(|f| f.is_seen()));
        assert!(vanished_ids.is_empty());
    }

    #[test]
    fn deleted_message_surfaces_as_vanished() {
        let dir = tempdir().unwrap();
        let mut client = mk_client(dir.path());
        client.create_mailbox("inbox").unwrap();
        let id_a = client.add_message("inbox", &[], RAW_A.to_vec()).unwrap();
        let id_b = client.add_message("inbox", &[], RAW_B.to_vec()).unwrap();

        let prev = snapshot_with(&[(&id_a, &[]), (&id_b, &[])]);
        client.delete_message("inbox", &id_b).unwrap();

        let diff = diff_envelopes(&mut client, "inbox", Some(&prev)).unwrap();
        let EnvelopeDiff::Incremental {
            new_envelopes,
            flag_updates,
            vanished_ids,
            ..
        } = diff
        else {
            panic!("expected Incremental");
        };
        assert!(new_envelopes.is_empty());
        assert!(flag_updates.is_empty());
        assert_eq!(vanished_ids, vec![id_b]);
    }

    #[test]
    fn sync_two_after_initial_population_is_a_no_op() {
        use crate::sync::{cache::MessageEntry, diff::pairs_from_envelopes, hunk::EmailHunk};
        use io_email::envelope::Envelope as IoEnvelope;

        let dir = tempdir().unwrap();
        let mut client = mk_client(dir.path());
        client.create_mailbox("inbox").unwrap();

        let seen = Flag::from_iana(IanaFlag::Seen);
        let mut snapshot = MessageSnapshots::new();

        for (raw, mid, want_seen) in [
            (RAW_A, "a@example.org", false),
            (RAW_B, "b@example.org", true),
        ] {
            let flags = if want_seen {
                vec![seen.clone()]
            } else {
                vec![]
            };
            let new_id = client.add_message("inbox", &flags, raw.to_vec()).unwrap();

            let env = IoEnvelope {
                id: new_id.clone(),
                message_id: Some(mid.to_string()),
                flags: flags.iter().cloned().collect(),
                subject: String::new(),
                from: Vec::new(),
                to: Vec::new(),
                date: None,
                size: 0,
                has_attachment: None,
            };
            let pairs = pairs_from_envelopes(vec![env]);
            let (key, _) = pairs.into_iter().next().unwrap();

            let hunk = EmailHunk::Copy {
                source_side: crate::side::Side::Left,
                target_side: crate::side::Side::Right,
                mailbox: "inbox".into(),
                source_id: "irrelevant".into(),
                flags: flags.iter().cloned().collect(),
                content_key: key,
            };
            match &hunk {
                EmailHunk::Copy { content_key, .. } => assert_eq!(*content_key, key),
                _ => unreachable!(),
            }
            snapshot.insert(
                key.to_string(),
                MessageEntry {
                    id: new_id,
                    flags: flags.iter().cloned().collect(),
                },
            );
        }

        let diff = diff_envelopes(&mut client, "inbox", Some(&snapshot)).unwrap();
        let EnvelopeDiff::Incremental {
            new_envelopes,
            flag_updates,
            vanished_ids,
            ..
        } = diff
        else {
            panic!("expected Incremental");
        };
        assert!(
            new_envelopes.is_empty(),
            "sync 2 should not see any new envelopes (would force a header parse)",
        );
        assert!(
            flag_updates.is_empty(),
            "sync 2 should not see flag updates"
        );
        assert!(
            vanished_ids.is_empty(),
            "sync 2 should not see vanished ids"
        );
    }

    #[test]
    fn new_message_added_after_snapshot_is_parsed_once() {
        let dir = tempdir().unwrap();
        let mut client = mk_client(dir.path());
        client.create_mailbox("inbox").unwrap();
        let id_a = client.add_message("inbox", &[], RAW_A.to_vec()).unwrap();

        let prev = snapshot_with(&[(&id_a, &[])]);
        let id_b = client.add_message("inbox", &[], RAW_B.to_vec()).unwrap();

        let diff = diff_envelopes(&mut client, "inbox", Some(&prev)).unwrap();
        let EnvelopeDiff::Incremental { new_envelopes, .. } = diff else {
            panic!("expected Incremental");
        };
        assert_eq!(new_envelopes.len(), 1);
        assert_eq!(new_envelopes[0].id, id_b);
        assert_eq!(
            new_envelopes[0].message_id.as_deref(),
            Some("b@example.org"),
        );
    }
}
