---
cairn: change
id: report-pulled-changes
status: landed
created: 2026-08-02
---

# Report the remote flag changes a pull applied

## Why

A flag changed remotely (e.g. `\Seen` added on the server) synced correctly — the
store was updated — but the report said "already in sync" and never mentioned it.
The report is built from the post-pull projection, where a just-pulled flag change
reads `Clean` (the base already matches), so it produced no hunk. io-replica's
pull already emits the per-item change (`ReplicaSyncReport.events`, including
`FlagsChanged`), but neverest read only the `pulled`/`pushed` counters and threw
the events away. A remote-originated change was applied silently.

## What

Consume the pull's `events` and itemize the remote-originated ones. Before the
pull, `mailbox_spine` snapshots `handle → flags`; after, `itemize_pulled` diffs a
`FlagsChanged` event against that snapshot into precise add/remove flag hunks, and
turns a `Vanished` event into a delete hunk. (A new remote message is an `Added`
event but is already reported as a `Fetch` in the pull plan, so it is not
re-itemized.) Local→remote pushes (`itemize_single`) and the download plan
(`itemize_fetches`) are unchanged.
