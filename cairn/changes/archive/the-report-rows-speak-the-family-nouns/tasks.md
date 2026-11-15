---
cairn: tasks
change: the-report-rows-speak-the-family-nouns
---

# Tasks

- [x] `level` with pimalaya-cli's `CapabilityLevel` (src/cli/check.rs)
- [x] `row` on every queue row; `kind`, `queuedAt`, `attempts`, `producer` on the parked rows; `set-flags` (src/cli/drain.rs, src/sync/report.rs, src/offline/driver.rs, src/offline/remote.rs)
- [x] `sources` on the account rows of `sync` and `drain` and on the index refresh (src/cli/sync.rs, src/cli/drain.rs, src/offline/index.rs)
- [x] `RejectedWrite` on `ItemError` (src/sync/report.rs)
- [x] `sync` on the streaming printer (src/main.rs)
- [x] Unit tests on the JSON keys, the integration suites read `row` and `level`
- [x] CHANGELOG, MIGRATION.md
- [x] Fold into cairn/spec/sync.md, log, archive
