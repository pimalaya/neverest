---
cairn: tasks
change: the-merger-is-configured-once
---

# Tasks

- [x] `Config` gains document-level `contact: ContactConfig` and `calendar: CalendarConfig`, defaulting to unset.
- [x] The account's merger wins where set, the document's fills in where it is not, resolved where the account config is loaded so the call site still reads one value.
- [x] Test: a document-level merger reaches an account declaring none, and an account declaring one is not overridden.
- [x] config.sample.toml shows the document-level form as the one to write, and the per-account override as the exception.
- [x] README.md and CHANGELOG.md under `### Changed`.
- [x] Fold the delta into cairn/spec/sync.md; log; land.
