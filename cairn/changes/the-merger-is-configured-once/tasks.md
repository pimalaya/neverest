---
cairn: tasks
change: the-merger-is-configured-once
---

# Tasks

- [ ] `Config` gains a document-level `conflict: ConflictConfig`, defaulting to unset.
- [ ] The account's merger wins where set, the document's fills in where it is not, resolved where the account config is loaded so the call site still reads one value.
- [ ] Test: a document-level merger reaches an account declaring none, and an account declaring one is not overridden.
- [ ] config.sample.toml shows the document-level form as the one to write, and the per-account override as the exception.
- [ ] README.md and CHANGELOG.md under `### Changed`.
- [ ] Fold the delta into cairn/spec/sync.md; log; land.
