---
cairn: log
change: cross-mailbox-pipeline
landed: 2026-08-02
---

# Sync the account as three cross-mailbox phases, not a per-mailbox loop

`run_single` was a per-mailbox loop: each mailbox's spine ran on the primary
connection (the others idle) and its hydration ended at a `thread::scope` barrier
(connections waiting for the slowest body before the next mailbox). On a
many-mailbox account that idle added up — the observed "last mail blocking the
other connections" at each boundary.

Restructured into three account-wide phases so the pool stays saturated:

1. **Phase 1 — spine, parallel over mailboxes.** `open_side_contexts` opens W
   single-connection contexts (handshakes overlapped); W store handles are opened
   for the same source. `phase1_spine` work-steals the mailbox queue across W
   workers, each running `mailbox_spine` (pull + meta + itemize + push, minus
   hydrate) on its own connection + store handle, collecting `(handle, size)`
   targets. Network overlaps; writes serialise on the store's single-writer lock
   (the seam explicitly allows process-level serialization). Collections are
   pre-created serially first. Phase 1 is Meta-tier only — no objects/blobs — so
   concurrent handles touch disjoint per-collection rows. io-pimdir's busy_timeout
   was raised 5s→30s to absorb a burst of large first-sync meta writes contending
   on the write lock.
2. **Phase 2 — hydrate, one global pool.** `phase2_hydrate` chunks every mailbox's
   targets into largest-first per-mailbox batches, queues the biggest first (global
   largest-first), and work-steals across the connections through one queue —
   `select_cached` re-SELECTs as a worker crosses mailboxes, so no connection idles
   at a boundary. Bodies stream into the blob store (`hydrate_batch`); items are
   cached by `(collection, handle)`.
3. **Phase 3 — apply, serial, no network.** `phase3_apply` drives each mailbox's
   `Full` upgrade over `CachedFetchRemote`, which serves the Phase-2 bodies from
   cache (miss → real fetch). Only index writes; cross-mailbox blobs are safe
   because object rows exist only for applied mailboxes (GC never sees a
   not-yet-applied blob).

io-replica/io-pimdir/io-imap untouched: the parallelism is a `CachedFetchRemote`
wrapper and per-worker store handles over the existing seams. A dry run stops
after Phase 1. Progress is three phases: `Scanning (k/M)`, one global
`Downloading n%`, `Writing (k/M)`.

Verified live (Stalwart, 6 seeded + 3 default mailboxes, 350 messages incl. big
ones, 4 connections): first sync downloaded 350, 350 blobs, every unique marker
present exactly once, **0 blobs with mixed content** (global pool routes
correctly), **0 Meta ghosts**, 350 Full; concurrent Phase-1 writes clean (no Busy,
no corruption). Idempotent re-sync ("already in sync", 0 downloads). Dry run
reports the 350-hunk plan, downloads nothing. Incremental (5 new across two
mailboxes) fetched only the 5, 0 ghosts. fmt/clippy clean; 15 unit tests pass.

Spec updated: `sync` (ADDED "three account-wide phases"; MODIFIED "Hydration may
run concurrently, largest-first" — now one global account-wide phase).
