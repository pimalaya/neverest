---
cairn: change
change: cross-mailbox-pipeline
---

## ADDED Requirements

### Requirement: The one-source sync runs as three account-wide phases
The one-source (retain) sync SHALL run as three phases across the whole account,
not a per-mailbox loop, so the connection pool never idles at a mailbox boundary:

- **Phase 1 — spine (parallel over mailboxes).** A work-stealing pool of workers,
  each on its own IMAP connection *and* its own store handle, SHALL reconcile each
  mailbox's spine (pull + meta + itemize + push) and collect its bodies to hydrate
  (`handle` + size from the local envelope meta). The network overlaps across
  mailboxes; the store writes serialise on the store's single-writer lock (the
  seam sanctions process-level serialization), and every collection is pre-created
  serially first so no worker races lazy creation. Phase 1 is Meta-tier only (no
  objects/blobs), so concurrent handles touch only disjoint per-collection item
  rows.
- **Phase 2 — hydrate (one global pool).** Every mailbox's bodies SHALL be chunked
  into largest-first per-mailbox batches, the biggest batches queued first for a
  global largest-first order, and work-stolen across the connections through **one**
  queue: a worker finishing one mailbox's last batch immediately steals the next
  mailbox's (`select_cached` re-SELECTs across the boundary), so no connection
  idles at a mailbox edge. Bodies stream into the blob store; the fetched items are
  cached by `(collection, handle)`.
- **Phase 3 — apply (serial, no network).** Each mailbox's `Full` upgrade SHALL be
  driven over a cache-backed remote serving the Phase-2 bodies (a miss falls back
  to a real fetch); only index writes happen, single writer.

A dry run SHALL stop after Phase 1 (reporting the pull plan, downloading nothing).
Progress SHALL be reported as the three phases — `Scanning mailboxes (k/M)`, one
global `Downloading n% (done/total)` over every body, then `Writing (k/M)`.

## MODIFIED Requirements

### Requirement: Hydration may run concurrently, largest-first
Full-tier hydration SHALL fetch bodies in **batches** — one `UID FETCH <set>
(UID BODY.PEEK[])` streaming K bodies (`BATCH_SIZE`, default 64) in a single
response — so N bodies cost ~N/K round trips per connection rather than one round
trip per message. Each message is routed to its own streaming sink by the **UID
on its own FETCH line**, so an out-of-order server response still lands
correctly; a body line without a parseable UID SHALL fail the batch so the caller
falls back to per-message fetches rather than misroute. In the one-source sync,
hydration is a single account-wide phase (see "three account-wide phases"): bodies
are ordered **largest-first** globally using each item's size from the store meta
(no size probe), chunked into per-mailbox batches, biggest first, and work-stolen
across the pool over one queue with no per-mailbox barrier. On any batch error the
fetch SHALL fall back to per-message fetches; content-addressing makes the partial
retry idempotent. The pool is **persistent**: connections are opened up front and
kept for the run, their auth paid once. The budget defaults to 4, is configurable
per account (`connections`) and overridable by a `sync --connections` flag, and
SHALL stay under the backend's per-account connection cap. Body bytes stream
lock-free into the blob store; the engine serialises the index write on the
single-writer store afterwards. The largest-first order takes its sizes from the
store meta, never a server size probe.
