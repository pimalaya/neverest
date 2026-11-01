---
cairn: log
change: a-reset-keeps-pending-work
date: 2026-10-30
---

# A reset keeps pending work

Release blocker B4 of the pimdir stable-release review: `sync --reset` deleted the store unchecked, ran under `--dry-run` and reset everything under a filter.

## What landed

- src/cli/sync.rs: `--reset` reads the store first (`pending_work`: queued and parked actions; per collection conflicts, retained items, pending deletes, and items whose state no binding's base holds) and refuses while any is there, listing it and naming `--force`; a store it cannot read is refused the same way. `--force` (requires `--reset`) moves every file of the store but its lock into a sibling `<dir>.reset-<instant>` (`move_aside`), deleting nothing. A plain reset of an idle store drops index.db too. `--reset` conflicts with `--dry-run`, `-m`, `-x` and `-s`.
- src/offline/driver.rs, src/offline/state.rs: the stale-store and sidecar refusals name `sync --reset --force`.
- README.md, MIGRATION.md: what a reset refuses and where `--force` puts the store.
- Tests: `a_reset_is_refused_beside_a_dry_run_or_a_filter`, `a_reset_refuses_while_work_is_pending`, `a_forced_reset_moves_the_store_aside`, all three failing on the old code (the flags parsed, the queued action was deleted, nothing was moved aside).

## Capabilities moved

- sync: a reset never loses work no server has (added); a store the format outgrew is refused with its remedy (modified: `--reset --force`); the sidecar refusal names `--reset --force`.
