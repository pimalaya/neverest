---
cairn: change
id: cross-mailbox-pipeline
status: landed
created: 2026-08-02
---

# Sync the account as three cross-mailbox phases, not a per-mailbox loop

## Why

`run_single` is a per-mailbox loop: for each mailbox it runs the spine
(enumerate + meta + itemize + push) then hydrates its bodies over the connection
pool. That leaves connections idle at every mailbox boundary:

- The spine of each mailbox runs on the **primary connection only** — the other
  pool connections sit idle through every mailbox's enumerate/meta/push.
- Each mailbox's hydration ends at a **`thread::scope` barrier**: connections that
  drained their batches wait for the slowest worker's last body before the next
  mailbox starts.

On a many-mailbox account (nested `Archives.*`, `Sent`, `Notes`, …) this idle
adds up, and the user observes the tail of a mailbox blocking otherwise-free
connections.

## What

Restructure the one-source sync into three account-wide phases so the connection
pool stays saturated end to end:

1. **Phase 1 — spine (parallel over mailboxes).** A work-stealing pool of workers,
   each owning its own IMAP connection *and* its own `PimdirStore` handle, drains a
   mailbox queue and runs that mailbox's enumerate + meta + itemize + push,
   collecting its hydration targets (`(mailbox, handle, size)`). The network
   (enumerate, envelope fetch, flag push) overlaps across mailboxes; the store
   writes are serialised by a shared process-level write lock — the trait itself
   sanctions "process-level serialization" as a single-writer mechanism — so the
   `&self` reads run concurrently (WAL) while `&mut self` writes take the lock.
   Collections are pre-created serially first, so no worker races on lazy
   collection creation.

2. **Phase 2 — hydrate (one global pool).** Every mailbox's targets are chunked
   into largest-first per-mailbox batches (a batched `UID FETCH` is within one
   selected mailbox), pushed biggest-first onto **one** global queue, and
   work-stolen across the pool. A worker finishing one mailbox's last batch
   immediately steals the next mailbox's — `select_cached` re-SELECTs as it
   crosses boundaries — so there is no per-mailbox barrier and no idle tail. Bodies
   stream into the content-addressed blob store; the fetched items are collected
   into a `(collection, handle) → ReplicaFetchedItem` cache.

3. **Phase 3 — apply (serial, no network).** For each mailbox, drive io-replica's
   `Full` upgrade with a **cache-backed remote** whose `fetch` returns the
   Phase-2 items (blobs already on disk); a cache miss falls back to a real fetch.
   Only store index writes happen here — fast, single writer, no network.

io-replica, io-pimdir and io-imap are untouched: the parallelism is expressed
with a `SyncWriteStore` wrapper (serialises `write`, delegates reads) and a
`CachedFetchRemote` wrapper (serves `fetch` from the cache), both implementing the
existing seams. The pipeline is one-source (`run_single`) only; `run_dual` keeps
its structure.

## Progress

Three phase-level bars replace the per-mailbox one: `Scanning mailboxes (k/M)`,
then the headline `Downloading n% (done/total)` — one continuous account-wide bar
over every body — then `Writing (k/M)`. Largest-first ordering is global (biggest
batches stolen first), so the download bar still accelerates to a smooth finish.

## Risks / verification

- Concurrent `PimdirStore` handles: reads are WAL-concurrent; writes serialise on
  the shared lock (no `BEGIN IMMEDIATE` contention, no `Busy`). Verify against a
  multi-mailbox Stalwart: correctness (every body present, correctly linked), an
  idempotent re-sync (no re-fetch, no new `Meta` ghosts), and connection
  saturation across boundaries.
- Cache completeness: Phase 2 fetches exactly Phase 1's targets; Phase 3's
  `Full` upgrade requests the same not-yet-`Full` set. A miss falls back to a real
  fetch, so a Phase-2 gap is corrected, not lost.
