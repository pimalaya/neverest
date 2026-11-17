---
cairn: change
id: every-failure-reaches-the-result
status: landed
created: 2026-11-17
---

# Every failure reaches the result

## Why

CLI family analysis of 2026-11-15 (~/.claude/plans/pimalaya-cli-family-analysis-2026-11-15/plan.md), findings NEV-1 (HIGH) and NEV-10, re-checked on e00c9bb:

- NEV-1: a pair collection whose sync fails is logged and evented, never recorded (src/sync/driver/pair.rs:416-432). The exit status reads the report alone, so a CardDAV pair whose target answers 500 to its REPORT prints "Account pair is already in sync" and exits 0, against *A collection that failed to scan is reported, never only logged*. The one-source path records a `scan` hunk and exits 4 (src/sync/driver/spine.rs:131-151). A full hydration failing in a pair is evented the same way and lost.
- NEV-10: failures that only warn, the result left clean: the queue listing or the store's drain failing (src/sync/driver/queue.rs:17, :28), the parked and pending rows read after a drain defaulting to empty on an error so failed rows count as drained (queue.rs:55-62), a downloaded batch failing to land in the store (src/sync/driver/hydrate.rs:231), the conflict count failing so the run exits 0 instead of 3 (src/sync/driver/upkeep.rs:88). And `sync --queue-only` exits 0 when it parks rows where `sync` exits 3 for the same rows (src/cli/queue.rs:89-92).

## What

- A collection hunk `sync`, naming `source`, `target` and `collection`, records a pair collection whose sync failed, with its error. It is not a `scan`: the error may come from either side, the store or the merge, and what the collection reported before failing is already folded in.
- A report row `failures`, one entry a store step that failed outside any hunk: `step` (`queue`, `bodies`, `conflicts`), `collection` (`null` for a store-wide step) and the failure keys `code`, `error`, `sources`. Each makes the run incomplete (exit 4) and counts among the text output's errors. A pair's failed full hydration is a `bodies` failure.
- `sync --queue-only` exits 3 when it parks a row, as a sync does.

All of it changes the report schema and exit statuses, so it lands before v1.0.0.
