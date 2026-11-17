---
cairn: tasks
change: the-account-groups-by-kind
---

# Tasks

- [x] src/config/{mail,contact,calendar}.rs replace `ItemSyncConfig` and src/config/conflict.rs; `AccountConfig` gains `mail`, `contact`, `calendar`.
- [x] `conflict resolve --interactive` picks the merger by kind; mail refused.
- [x] `MsgraphTable::contact`, the domain named `contact` in refusals.
- [x] Test: a card is not handed to the calendar's merger (fails when cards read `calendar.merger`).
- [x] config.sample.toml, CHANGELOG.md, tests/{merger,scope,google,msgraph}.rs, tests/common.
- [x] Fold into cairn/spec/sync.md; the-merger-is-configured-once and jmap-sources updated to the new keys; log.
