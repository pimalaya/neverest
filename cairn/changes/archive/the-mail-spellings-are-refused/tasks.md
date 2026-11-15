---
cairn: tasks
change: the-mail-spellings-are-refused
---

# Tasks

- [x] src/config.rs: the `mailbox`, `message` and `filters` aliases dropped; `AccountMailboxRemoved`, `AccountMessageRemoved`, `SourceMailboxRemoved`, `SourceMessageRemoved` and `CollectionFiltersRemoved` declared by `pimalaya_config::removed_key!` and read as removed fields
- [x] Tests: `a_removed_key_points_at_the_migration_notes` gains eight cases (failing on the aliases, which loaded, and on the account-level `mailbox`, which read as `collection`); `the_pre_generic_pim_sync_spellings_still_load` becomes `the_generic_pim_sync_spellings_load`, the alias half moved to the refusals
- [x] CHANGELOG (Removed, upgrade list), MIGRATION.md (next release, and the v0.2.0 highlight saying the aliases lasted one release)
- [x] Fold into cairn/spec/sync.md, log, archive
