---
cairn: log
change: a-move-pushed-before-its-target-is-itemized
date: 2026-11-23
---

# A move pushed before its target is itemized reports its create once

Found by the live test night of 2026-11-23 (.lanes/live/journal.md): a message moved to the trash through himalaya's pimdir queue was reported as two adds in Deleted Items, on Graph (`neverest --json sync -m live-2026-11-23-nv -m inbox -m "Deleted Items"`, three runs out of three) and on Stalwart (once in two runs). The server and the store held one copy each: the report alone was wrong.

Phase 1 spines collections in parallel. The collection the item leaves reports the create beside its remove, then pushes the remove, which drops its tombstone; the collection it enters, itemized after that, no longer finds the tombstone carrying the create, so it reports the create again.

## What landed

- src/sync/hunk.rs: `ItemHunk::create_key`, the target side, collection and key of an `add` or a `copy`.
- src/sync/report.rs: `SyncOutput::absorb` folds a collection's hunks through `absorb_item_patch`, which drops a create the report already holds.
- src/sync/driver/tests.rs: `a_queued_move_itemized_across_its_push_reports_its_create_once` itemizes INBOX, drops its tombstone as the push does, itemizes Archive and folds both reports; it counted two adds before the fix.

## Spec

[sync](../spec/sync.md): the requirement on a relocation's create names the push case and the one create per side, collection and key; scenario *A queued move itemized across its push*.
