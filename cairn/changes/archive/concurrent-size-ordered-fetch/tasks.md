---
cairn: tasks
change: concurrent-size-ordered-fetch
---

# Tasks

- [ ] A bounded worker pool per side, one connection per worker, size capped to
      the backend's connection limit.
- [ ] Service the engine's Full-fetch batch across the pool, whole-message jobs,
      streaming each body per `object-bytes-by-reference`.
- [ ] Schedule largest-first using the enumerated member size (from io-replica
      `concurrent-size-ordered-fetch`).
- [ ] Confirm index writes serialise on the single-writer store while bodies
      stream lock-free (no store contention on the byte path).
- [ ] Connection lifecycle (open/auth/reconnect) for the pool.
- [ ] Tests: a heavy message overlaps light ones; results matched by handle.
- [ ] Fold spec: `sync`. Log entry.
