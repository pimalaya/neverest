---
cairn: tasks
change: a-rename-has-its-switch
---

# Tasks

- [x] `collection.rename: Option<bool>` (src/config.rs), `CollectionPermissions::renames`
- [x] src/offline/capability.rs: rename declared `none` when the switch is off, the detail naming the key
- [x] Test `a_rename_follows_its_switch_and_a_one_way_account_is_off` (one-way default refuses, explicit true allows, two-way default allows, explicit false refuses); `a_delete_is_declared_only_where_collection_delete_is_granted` reads the one-way rename as `none`
- [x] config.sample.toml, CHANGELOG, MIGRATION; fold into cairn/spec/sync.md, log
