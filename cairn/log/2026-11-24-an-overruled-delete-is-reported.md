---
cairn: log
change: the-live-night-findings
landed: 2026-11-24
---

# An overruled delete is reported

Task N2 of the-live-night-findings, under the owner's ruling R2 of 2026-11-24: a delete beaten by a server edit is a report line, not a conflict. The same ruling closes the rest of N3: no wait after a Graph body write, the overruled delete is reported. On the live night of 2026-11-23 a queued delete beaten by a server edit (pimdir SYNC §5) read "applied 1 queued action(s)", the event back with no word.

`itemize_overruled_deletes` (src/sync/driver/itemize.rs) compares the tombstones a side held before its pull (`tombstoned`) with the sync's events: a tombstone an `Added`, `ContentChanged` or `FlagsChanged` event names, live and not conflicted after the sync, is a new `ItemHunk::DeleteOverruled` (JSON `kind: "delete-overruled"`, text `delete of item <id> in <collection> overruled by a server edit on <side>`), named by its link id. io-pimdir revives a DAV tombstone as an `Added` member. Called from the one-source spine (src/sync/driver/spine.rs) and the pair's pull pass (src/sync/driver/pair.rs, one load serving the flag snapshot and the tombstones). Capability moved: sync.md, new "A delete a server edit overruled is reported".

## Checked

- tests/pair.rs `a_delete_a_server_edit_overrules_is_reported` (fake CardDAV: a card synced, a `Remove` queued, the card edited on the server): before the fix `drained` read `applied: 1`, the card stayed and `itemPatch` held only `fetch item a` (`left: 0 right: 1`); after it, one `delete-overruled` hunk, said once, and the text line.

Not run live: no frontend at hand queues a card delete on the lane's Stalwart store; the fake server takes the same code path.
