---
cairn: tasks
change: sync-progress-is-events
---

# Tasks

- [x] `SyncEvent` type, registered as `neverest-sync.events`
- [x] `Progress` handle: spinner in text mode, events under `--json`; `PercentGate` once per percent, reopened by a new count
- [x] Thread it through `driver::run`, the pair and the one-source paths, phase 1 and phase 2
- [x] Registry on `JsonSchemaRegistry::new` and `insert`, `check` test with bare `neverest` silent
- [x] Integration tests read the result line of `sync --json`
- [x] Tests: the gate, a download printing its progress as events
- [x] CHANGELOG
- [x] Fold into cairn/spec/sync.md; log; land
