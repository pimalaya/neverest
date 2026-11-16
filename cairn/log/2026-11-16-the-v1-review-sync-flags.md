---
cairn: log
change: the-v1-review
date: 2026-11-16
---

# The v1 review, phase 4: `sync` flags

- `sync --init` initializes the accounts not yet initialized (src/cli/init.rs `initialize`, shared with `init`), then syncs; the result names them under `initialized`.
- `sync --queue-only` is the former `drain` (src/cli/queue.rs), `sync --index-only` the former `index` (src/cli/index.rs); both commands are gone. Their rows fold into the sync result under `queue` and `index`, so `neverest-sync` stays the one schema (the registry refuses a key naming no command).
- The two `-only` flags exclude each other, `--init` and the full-sync flags (clap `conflicts_with_all`, exit 2).
- The not-initialized message names `neverest init -a <name>` and `neverest sync --init -a <name>`.
- Tests: tests/accounts.rs (`--init` on initialized and uninitialized accounts, the flag conflicts, `--index-only`); tests/drain.rs became tests/queue.rs (busy store included).
- Capability moved: sync (`sync` initializes, queues or indexes on a flag).
