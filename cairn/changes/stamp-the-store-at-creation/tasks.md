---
cairn: tasks
change: stamp-the-store-at-creation
---

- [x] `StoreState::stamp` writes a default sidecar at the current layout
- [x] `neverest init` stamps the store it materializes
- [x] `reset_replica` stamps the store it recreates
- [x] Test: a stamped store is not taken for the unnamespaced ancestor
- [x] Test: stamping forgets what a previous store derived
- [x] Test: a reset leaves a store the next run can read
