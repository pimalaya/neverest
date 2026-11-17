---
cairn: tasks
change: every-failure-reaches-the-result
---

# Tasks

- [x] Tests first, failing on e00c9bb: a pair whose target fails its listing exits 4 with one `sync` hunk; a drain and a conflict count that fail each leave a `failures` row and exit 4; `--queue-only` parking a row exits 3.
- [x] `CollectionHunk::Sync`; `run_pair` records a failed collection with it and a failed full hydration as a `bodies` failure.
- [x] `SyncOutput::failures` (`StepFailure`, `FailedStep`): absorbed, counted by `incomplete()`, printed among the errors.
- [x] queue.rs, hydrate.rs, upkeep.rs record their failures; the post-drain reads propagate instead of defaulting.
- [x] `--queue-only` exits 3 on a parked row.
- [x] Fold the delta into spec/sync.md, log, status landed.

Not done: no test provokes a batch failing to land (src/sync/driver/hydrate.rs), nor a pair's full hydration failing; both record through the same `StepFailure` path the tested sites use. The queue-only exit 3 test was not run against e00c9bb (the old exit reads `busy || error` alone, src/cli/queue.rs:89-92 there).
