---
cairn: tasks
change: persistent-fetch-pool
---

# Tasks

- [x] `Pool` type (primary + lazily-grown, reused across the run) in `client`.
- [x] `EmailRemote` wraps `&mut Pool`; sequential verbs on the primary, `Full`
      fetch distributes the pool's own connections into the workers.
- [x] Per-account `connections` config + `sync --connections/-j N` flag, default 4.
- [x] Verify Stalwart e2e (five varying-size messages through the persistent pool).
- [x] Fold spec `sync`; log.
