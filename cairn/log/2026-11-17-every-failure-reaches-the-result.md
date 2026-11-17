---
cairn: log
change: every-failure-reaches-the-result
date: 2026-11-17
---

# Every failure reaches the result

Findings NEV-1 and NEV-10 of the CLI family analysis of 2026-11-15, re-checked on e00c9bb.

## What landed

- A pair collection whose sync fails is recorded as a `CollectionHunk::Sync { source, target, collection }` with its error (src/sync/driver/pair.rs), so the run exits 4 and the text no longer says "already in sync". It was logged and evented only, the run exiting 0.
- `SyncOutput::failures` (`StepFailure`, `FailedStep`: `queue`, `bodies`, `conflicts`) records a store step failing outside any hunk: the queue listing or drain (src/sync/driver/queue.rs), the parked rows listing and the conflict count (src/sync/driver/upkeep.rs), the store failing to open after the run (src/sync/driver.rs), a downloaded batch failing to land (src/sync/driver/hydrate.rs, through `Noted`), a pair's full hydration. `incomplete()` reads it (exit 4); the text prints it among the errors.
- The rows read after a drain propagate their errors, where an error read as an empty set counted failed rows as drained.
- `drain_store` fails on a store step; `sync` records it as a `queue` failure for each account, `sync --queue-only` fails each account of the store (exit 4).
- `sync --queue-only` exits 3 when it parked a row.
- Tests: tests/pair.rs over a new fake CardDAV server (tests/fakedav/), failing on e00c9bb with exit 0 and an empty patch; `a_drain_that_fails_is_reported_and_the_run_is_incomplete` and `a_conflict_count_that_fails_leaves_the_run_incomplete` on a store with a table dropped; tests/queue.rs `a_drain_that_parks_a_row_exits_3`.
- Not tested: a batch failing to land and a pair's full hydration failing.

## Capabilities moved

- [sync](../spec/sync.md): *A failed pair collection is reported* and *A store step that fails is reported* added; *A drain applies the queue without syncing*, *An incomplete run has its own exit code* and the failure-row list amended.
