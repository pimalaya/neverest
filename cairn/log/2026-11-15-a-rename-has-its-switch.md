---
cairn: log
change: a-rename-has-its-switch
date: 2026-11-15
---

# A rename has its switch

Ledger line N14 of the CLI alignment round of 2026-11-15 (owner decision D14, from live testing on Fastmail).

## What landed

- src/config.rs: `collection.rename`, optional, and `CollectionPermissions::rename` with `renames(one_way)`: unset reads the account's `one-way`.
- src/offline/capability.rs: a rename the switch refuses is declared `none`, its detail naming the key.
- Tests: `a_rename_follows_its_switch_and_a_one_way_account_is_off`; `a_delete_is_declared_only_where_collection_delete_is_granted` now reads a one-way rename as `none`.
- config.sample.toml documents the key.

## Capabilities moved

- [sync](../spec/sync.md): *Every source declares its capabilities before the drain* and *A collection is renamed or deleted through the queue* amended.
