---
cairn: tasks
change: every-key-is-printed
---

# Tasks

- [x] `AccountMode` derives `Serialize` and `JsonSchema`; `check` and `init` print it as `mode`
- [x] Drop every `skip_serializing_if` on printed types (src/sync/report.rs, src/sync/hunk.rs, src/cli/drain.rs, src/cli/check.rs); docs say `null`
- [x] Tests asserting an omitted key expect `null`, `[]` or `false`
- [x] CHANGELOG
- [x] Fold into cairn/spec/sync.md; log; land
