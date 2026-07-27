---
cairn: change
id: concurrent-size-ordered-fetch
status: landed
created: 2026-07-31
---

# Concurrent, size-ordered hydration

## Why

Hydration is sequential over one connection per side, so one heavy message stalls
the tail of a sync — worst when it is processed last with nothing to overlap. An
**optimisation** (throughput / head-of-line blocking), not a correctness fix.

Depends on [`object-bytes-by-reference`]: N concurrent fetches without streaming
would multiply memory (N × full body). Streaming lands first, then concurrency is
safe (total ≈ pool_size × chunk buffer).

The unit of concurrency is **one whole message on its own connection**, not a
chunk: a single message is one ordered literal on one socket (FETCH) and one on
the target (APPEND) — unsplittable across workers. Parallelism comes from
multiple connections.

## What

- A bounded pool of connection-owning workers services the Full-fetch batch that
  the engine yields (`WantsFetch` is order-independent, results keyed by handle).
- Jobs are scheduled **largest-first** by the enumerated member size (io-replica
  surfaces it), so the heavy message overlaps the light ones instead of trailing.
- Body bytes stream lock-free into the blob store; only the small index commit
  serialises on the single-writer store.

## Scope / non-goals

- Depends on `object-bytes-by-reference` (this repo, io-pimdir, io-replica) and on
  io-replica `concurrent-size-ordered-fetch` (member size + fetch order-
  independence).
- Pool size SHALL NOT exceed the backend's connection limit (~a handful), so the
  win is overlap, not unbounded fan-out.
- No chunk-level tasking; no async reactor (that is gateway scale — pimgate /
  carillon — not a single-account sync). Not real-time / watch.
