---
cairn: change
id: a-capped-source-says-so
status: landed
created: 2026-11-13
---

# A capped source says so

## Why

pimdir audit line Y4 (pimdir cairn/changes/audit-2026-11-12/tasks.md): no neverest connector could state `capped` and no configuration key existed, so pimdir SYNC §5's rule for a capped listing never fired. Gmail over IMAP with its folder size limit set after 1,900 messages synced lists the newest 1,000: the 900 others are absent from every round, under the half that parks a mass absence, and were read as removed.

## What

- A per-source key, `<backend>.collection.capped` (boolean, false by default), in the `collection` table every backend already carries.
- The remote seam states `capped` on every page of a round of such a source, and on no delta nor head (pimdir SYNC §4).
- The report lists under `unlisted`, per collection, the count of based bindings a capped round did not list (the engine's `unlisted`), printed under `Unlisted`: no warning, no exit code.
- config.sample.toml, README and CHANGELOG state the key and what it costs: on a server stating no removal, a message deleted there stays in the store.

## Why this key

- **Under `collection`**: the table holds what a source states of its collections (`filter`, `create`, `delete`), the cap is a fact of how the source lists a collection, and the table exists on every backend, so the key needs no new table and no per-backend code.
- **`capped`**: the standard's own word, and the page field's.
- **A boolean**: neverest needs no number. The engine counts what a round left unlisted, and no connector can compare a limit with what a scoped round lists. A number would invite reading "listed fewer than the limit" as "not capped", which a round over a scope does not prove.
- **Per source, not per collection**: the limit the audit names is one setting for every folder of the account.
