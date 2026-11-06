---
cairn: log
change: every-key-is-printed
date: 2026-11-03
---

# Every key is printed

Every `skip_serializing_if` on a printed type is gone (src/sync/report.rs, src/sync/hunk.rs, src/cli/drain.rs, src/cli/check.rs): an empty list prints `[]`, an absent value `null`, a false `default` `false`, and the schemas mark those keys required. `AccountMode` (src/config.rs) derives `Serialize` and `JsonSchema` and is the `mode` of `check` and `init`, `{sources, targets, oneWay, retain}`, its sentence kept as the text form. The sidecar state (src/offline/state.rs) keeps its omissions, being read back rather than printed.

Capabilities moved: sync (cairn/spec/sync.md: "Every key is printed" and "The mode is data" added; "A check lists collections with their roles", "A check names the default calendar and address book" and "A check reports what each source declares it can do" modified to say `null` and `false`). The tests that asserted an omitted key (src/cli/exit.rs, src/offline/driver.rs, src/cli/check.rs, tests/intents.rs, tests/duplicates.rs, tests/heads.rs, tests/scope.rs, tests/check.rs) now expect `null`, `[]` or `false`.

This closes neverest's ledger line in pimalaya-cli every-output-is-data, with sync-progress-is-events.
