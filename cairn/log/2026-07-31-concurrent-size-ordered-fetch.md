---
cairn: log
change: concurrent-size-ordered-fetch
landed: 2026-07-31
---

# Concurrent, size-ordered hydration (neverest part)

Hydration now fetches bodies concurrently, largest-first, so one heavy message
overlaps the light ones instead of stalling the batch. `hydrate_copies` batches
each side's targets into one `Full` upgrade (was one drive per handle).
`EmailRemote` gained the side's `SideConfig` and a worker count; `fetch_full`
lists envelope sizes once, sorts the handles largest-first (longest-processing-
time), and — when the batch is worth it (`workers = min(FETCH_WORKERS=4, n) > 1`)
— fans out across a bounded scoped-thread pool. Each worker opens its own
connection from the config and drains a shared FIFO queue seeded in size order,
streaming each body into the blob store per `object-bytes-by-reference` (the blob
writer's per-write temp name makes concurrent bodies collision-free). A trivial
batch stays serial on the primary connection. Body bytes stream lock-free; the
engine serialises the one index write afterwards.

Verified end-to-end: the Stalwart roundtrip now seeds five messages of varying
size (one ~3 MB), and all five round-trip m2dir A → IMAP → m2dir B through the
pool. Unit tests and fmt pass.

Depends on io-replica `concurrent-size-ordered-fetch` (fetch order-independence)
and `object-bytes-by-reference` (streaming bodies).

Follow-ups: the pool is ephemeral (opens connections per Full batch — a
persistent pool would amortise auth); `FETCH_WORKERS` is a constant, not yet
configurable; m2dir workers read whole files (no native chunked stream yet).

Spec updated: `sync` (ADDED: hydration may run concurrently, largest-first).
