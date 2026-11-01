---
cairn: tasks
change: the-sidecar-is-written-whole
---

# Tasks

- [x] `offline/state`: `StoreState::save` writes a temporary file, syncs it and renames it over neverest.json.
- [x] Test, failing first: a save replaces neverest.json by a rename (a new inode on Unix), leaves nothing beside it, and the saved sidecar loads.
- [x] Spec folded, log written, CHANGELOG.
