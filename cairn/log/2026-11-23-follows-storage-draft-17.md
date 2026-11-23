---
cairn: log
change: follows-storage-draft-17
date: 2026-11-23
---

# neverest follows storage-draft-17

neverest moves to io-pimdir cc3021b (pimdir storage-draft-17, sync-draft-15, search-draft-17) with the four other CLIs, closes a silent loss on its reset, and stops listing what the store and the source both say is unchanged. Plan, evidence and decisions: [the change](../changes/follows-storage-draft-17/), its journal.md the timed record.

## What landed

- Cargo.toml, Cargo.lock: the io-pimdir git pin b080e14 to 7f16b27, then to cc3021b (drop 1 of the io-pimdir lane), the lock's source line alone (D1); url 2.5.9 for RUSTSEC-2026-0592 (D3). src/sync/driver/tests.rs `a_store_version_tells_an_earlier_neverest_from_a_newer_one` names draft-14 to draft-16 earlier, the crate's own draft and draft-18 not.
- G1, silent loss: src/cli/sync.rs `pending_work` counts the live items holding a local property, so a plain `sync --reset` refuses over them naming `--force` (D2). Test `a_reset_refuses_while_an_item_holds_a_property`, failing before.
- E: src/sync/driver/tests.rs `a_capped_renumbered_mailbox_parks_on_one_absence` pins io-pimdir's capped rekey park; it fails with `with_capped(false)` at src/sync/driver/reconcile.rs.
- F1: src/sync/driver/reconcile.rs `owes_no_push` reads `settled_collections` where a push pass would start; src/sync/driver/spine.rs and src/sync/driver/pair.rs skip the push passes of a settled collection. Tests `a_settled_collection_is_listed_once_a_run` and its pair twin, 2 `SELECT`s before, 1 after; the fake IMAP server answers `LIST`.
- F2: src/sync/driver/local.rs `quiet_collections` before phase 1, through `Client::quiet_collections`: Gmail by one history read (src/gmail/client.rs `quiet_labels`, a quiet label's checkpoint moved in one write), DAV by the sync token io-webdav already reads in the home-set listing (src/dav/client.rs). Tests src/sync/driver/gmail_quiet.rs and tests/pair.rs `a_book_whose_sync_token_did_not_move_is_not_listed`; tests/fakedav states the token.
- Server suites: tests/multikind.rs and tests/merger.rs assert the conflict rows of the 2026-11-20 rulings, stale since then.

## Parked

- F2 over IMAP (step 11): io-imap 0.12.0 sends no `LIST ... RETURN (STATUS ...)`; an io-imap ask.
- F3 (step 12): a Gmail body linked from a twin `Message-ID` was not reproduced at cc3021b; src/sync/driver/gmail_bodies.rs `two_messages_of_one_message_id_keep_their_own_bodies` pins the property, no production change.

## Spec

[sync](../spec/sync.md): the header names storage-draft-17 and search-draft-17; *The summary is the format's typed row* names the one `summary` table; *A reset never loses work no server holds* counts local properties; the capped rekey parks on one absence; `property-invalid` joins the codes neverest cannot meet; new *A settled collection is listed once a run* and *A source that states no change leaves a settled collection unsynced*.
