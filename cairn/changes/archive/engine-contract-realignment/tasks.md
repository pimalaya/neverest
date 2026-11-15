---
cairn: tasks
change: engine-contract-realignment
---

- [x] `ReplicaChangeKind` at the four push sites and the two test matches
- [x] `PimdirPurgeReport` no longer carries bytes
- [x] `sync_options` with `ReplicaDeletePolicy::Keep`, used by the run and the tests
- [x] Test: a side that may not delete keeps the tombstone (fails under `Revert`)
- [x] `sweep_retained` collects after a purge, and reports what was freed
- [x] Test: a purged item's body actually leaves the blob tree
