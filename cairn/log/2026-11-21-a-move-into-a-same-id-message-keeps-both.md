---
cairn: log
change: a-move-into-a-same-id-message-keeps-both
date: 2026-11-21
---

# A move into a same-ID message keeps both

The neverest half of io-pimdir cairn/changes/a-same-id-message-keeps-its-body (pimdir SYNC §4 and §6, sync-draft-15 and storage-draft-15 edited in place): a relocation names the member it made.

## What landed

- src/imap/backend.rs: `move_messages` answers the `(source, destination)` UID pairs of the `COPYUID` code `UID MOVE` or `UID COPY` returns (`moved_uids`), where it dropped them; a code naming more UIDs than the move answers none. Test `a_move_answers_the_uids_its_copyuid_states`.
- src/client.rs: `Client::move_items` answers those pairs; Graph, DAV and Gmail answer none.
- src/sync/remote.rs: `relocated` sets the outcome's `assigned` to the UID the code pairs with the moved one, which io-pimdir lands the target's create on.
- tests/moves.rs: repro.patch, `a_move_into_a_message_stating_its_id_keeps_both`, ignored without Stalwart.
- Building against io-pimdir's working tree: `search` enables `io-pimdir/indexer` too (`PimdirIndex::open` and `refresh` moved behind it), src/sync/report.rs names `PimdirAction::SetProperty`, and every `PimdirPlacement` and `PimdirRemoteMeta` literal states `server_object_id` (none: no connector here states one). Cargo.toml patches io-pimdir to the working tree under `[patch.crates-io]` until io-pimdir releases.

## Results

Stalwart v0.16 (tests/stalwart2.sh, tests/pasta.sh): the unlisted variant passes, and fails with `relocated` given no pairs (`MOVEDMARKER` bound to the other message's UID). The listed variant fails: B's item holds A's body, io-imap 0.12.0 offering no `EMAILID` for neverest to state though Stalwart announces `OBJECTID`; the test documents that gap. `cargo test --all-features --no-fail-fast` 725 passed, 0 failed, 72 ignored; clippy and fmt clean.

## Spec

[sync](../spec/sync.md) gains *A relocation names the member it made*.
