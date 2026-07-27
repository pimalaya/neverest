---
cairn: change
id: largest-first-restore
status: landed
created: 2026-08-01
---

# Restore largest-first hydration order (from store meta, no probe)

## Why

Batching the body fetch dropped the largest-first ordering and hydrated in UID
order. That made the progress bar move *linearly* — which, in practice, felt
slower and, worse, sometimes **froze near the end** when a large message happened
to land at a high UID: the tail of the sync stalled on one big body while the
counter sat at ~97%. The old largest-first order put the heavy messages up front,
so the counter crawled at the start and *accelerated to a smooth finish* — both
more satisfying and free of the end-stall.

The original largest-first paid for a size probe (a `UID FETCH … RFC822.SIZE`
round trip), which is why batching had removed it. But the sizes are already local:
the `Meta` tier fetched each message's envelope (including `RFC822.SIZE`) and wrote
it to the store meta before hydration. So the order can be restored for free.

## What

The driver reads each not-yet-`Full` item's body size from its store meta (the
`v:1` summary's `size`) while collecting the hydration handles — no server round
trip — and passes a `handle → size` map into the `Full` fetch. `fetch_full` then
orders **largest-first** when sizes are present (heavy messages front-loaded,
accelerating finish, no end-freeze), falling back to UID order when they are not
(e.g. the two-source cross-copy path, which passes an empty map). Batches are still
chunked from the ordered handles and work-stolen across the pool, so the biggest
batches start first. No size probe is reintroduced.
