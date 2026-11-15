---
cairn: change
change: a-reset-keeps-pending-work
---

# Delta

## ADDED Requirements

### Requirement: A reset never loses work no server holds
`sync --reset` SHALL read the store before dropping it and SHALL refuse while it holds work no server has: a queued or parked action, a conflict, an item whose flags or body no binding's base holds (a staged edit or create), a staged delete, or a retained item, bound to no server. The refusal SHALL list that work per collection and name `--force`. A store holding none of it SHALL be dropped and recreated, its search index with it.

`--reset --force` SHALL NOT delete anything: it SHALL move the store (the database and its WAL files, the objects, the search index and the sidecar) into a sibling directory named `<account>.reset-<UTC instant>`, then recreate the store. A store that cannot be read, written by an earlier draft or a newer neverest, SHALL be reset this way only.

`--reset` SHALL be refused beside `--dry-run`, which would drop the real store before planning, and beside a collection or source filter, the reset being the whole store's.

#### Scenario: A queued send survives a reset
- **GIVEN** a store holding a queued action
- **WHEN** the account is synced with `--reset`
- **THEN** the run refuses, naming the queued action and `--force`, and the store is untouched

#### Scenario: A forced reset moves the store aside
- **GIVEN** a store holding a queued action
- **WHEN** the account is synced with `--reset --force`
- **THEN** the old store is in a sibling `reset-` directory, whole, and the account syncs into a fresh one

## MODIFIED Requirements

### Requirement: A store the format outgrew is refused with its remedy
A store an earlier draft of the pimdir format wrote cannot be migrated in place. Opening one SHALL fail naming `sync --reset --force` for the account, the command that moves the store aside and resyncs it, rather than surfacing the raw refusal. What the old store held that no server holds (queued actions, staged edits, retained items) stays in the store moved aside.

## REMOVED Requirements

None.
