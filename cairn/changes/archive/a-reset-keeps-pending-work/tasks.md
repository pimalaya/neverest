---
cairn: tasks
change: a-reset-keeps-pending-work
---

# Tasks

- [x] `cli/sync`: `--force` (requires `--reset`); `--reset` conflicts with `--dry-run`, `-m`, `-x`, `-s`.
- [x] `cli/sync`: the pending work of a store (queued and parked actions, conflicts, placements not `Clean`, retained items), listed in the refusal.
- [x] `cli/sync`: `--force` moves the store aside into `<account>.reset-<instant>`; a plain reset removes index.db too.
- [x] `offline/driver`: the stale-store message names `--reset --force`.
- [x] Tests, failing first: a reset of a store with a queued action refuses and keeps it; a forced reset moves it aside, nothing deleted; `--reset --dry-run`, `-m`, `-s` refused; a reset of an idle store resets it.
- [x] Spec folded, log written, CHANGELOG, README and MIGRATION.
- [x] Verify: `cargo fmt`, `cargo clippy --all-features --all-targets -- -D warnings`, `cargo test --all-features`.
