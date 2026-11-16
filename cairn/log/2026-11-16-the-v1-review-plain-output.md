---
cairn: log
change: the-v1-review
date: 2026-11-16
---

# The v1 review, phase 2: plain output

- pimalaya-cli is taken with `terminal` and `signal`, not `spinner`: crossterm leaves the dependency tree.
- `sync` in text mode prints its `SyncEvent`s as plain lines on stderr, the per-percent ones left out (`SyncEvent::is_coarse`); stdout keeps the result.
- `check`, `init`, `index` and the credential resolution print one plain line a step through src/notice.rs, silenced under `--json`.
- Tests: a text-mode sync prints plain lines on stderr with no escape byte (tests/accounts.rs); a download renders its start and end alone in text mode.
- Capability moved: sync (text mode prints plain lines; a sync prints its progress as events).
