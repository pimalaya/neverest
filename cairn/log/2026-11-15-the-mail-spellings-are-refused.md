---
cairn: log
change: the-mail-spellings-are-refused
date: 2026-11-15
---

# The mail spellings are refused

Line N8 of the CLI alignment round of 2026-11-15 (audit finding F38).

## What landed

- src/config.rs: the serde aliases `mailbox` (of a backend table's `collection`), `message` (of a backend table's `item` and of the account's `item`) and `filters` (of `collection.filter`) are gone. `AccountMailboxRemoved`, `AccountMessageRemoved`, `SourceMailboxRemoved`, `SourceMessageRemoved` and `CollectionFiltersRemoved`, declared by `pimalaya_config::removed_key!`, refuse them by name as the file is parsed; the account-level `mailbox` no longer reads as `collection` in its refusal.
- Tests: `a_removed_key_points_at_the_migration_notes` gains eight cases (the account's `mailbox` and `message`, a backend's `mailbox` under the account and under `sources`, its `message` under the account and under `targets`, `collection.filters` as a string and as a table), each loading or naming `collection` before the change; `the_pre_generic_pim_sync_spellings_still_load` becomes `the_generic_pim_sync_spellings_load`.
- CHANGELOG: a **BREAKING** Removed entry and an upgrade line. MIGRATION.md: a before and after under the next release; the v0.2.0 highlight says the aliases lasted through v0.3.0.

## Capabilities moved

- [sync](../spec/sync.md): *The configuration refuses every key it does not read* names the new keys, states that no key is read under an old spelling, and gains the scenario *A v0.2 mail spelling*.
