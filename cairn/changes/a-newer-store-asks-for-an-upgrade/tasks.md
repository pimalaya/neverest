---
cairn: tasks
change: a-newer-store-asks-for-an-upgrade
---

# Tasks

- [x] `offline/driver`: `open_store` answers `Version` with an upgrade message, `Stale` with `sync --reset --force`.
- [x] Test, failing first: a store whose `user_version` is above io-pimdir's is refused with the upgrade message and no reset.
- [x] Spec folded, log written, CHANGELOG.
