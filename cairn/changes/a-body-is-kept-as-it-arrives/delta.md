---
cairn: change
change: a-body-is-kept-as-it-arrives
---

# Delta

## ADDED Requirements

### Requirement: The one-source sync runs as two account-wide phases
The one-source (retain) sync SHALL run as two phases across the whole account, not a per-mailbox loop, so the connection pool never idles at a mailbox boundary:

- **Phase 1, spine (parallel over mailboxes).** A work-stealing pool of workers,
each on its own IMAP connection *and* its own store handle, reconciles each mailbox's spine (pull + meta + itemize + push) and collects its bodies to hydrate (`handle` + size from the local envelope meta). The network overlaps across mailboxes; store writes serialise on the store's single-writer lock (the seam sanctions process-level serialization), and every collection is pre-created serially first so no worker races lazy creation. Phase 1 is Meta-tier only (no objects/blobs), so concurrent handles touch only disjoint per-collection rows.
- **Phase 2, hydrate and apply (one global pool).** Every mailbox's bodies are chunked into
largest-first per-mailbox batches, biggest batches queued first for a global largest-first order, and work-stolen across the connections through **one** queue: a worker finishing one mailbox's last batch immediately steals the next mailbox's (`select_cached` re-SELECTs across the boundary), so no connection idles at a mailbox edge. Bodies stream into the blob store. The worker that fetched a batch SHALL raise its items to `Full` before taking the next one, over a cache-backed remote serving that batch alone (a miss falls back to a real fetch on the worker's connection). The applies SHALL be serialised on one store handle, the same body possibly belonging to two mailboxes.

A body SHALL be readable from the store as soon as its batch is applied, not once the account has downloaded. A run that stops during phase 2 SHALL keep every batch it applied, so the next run's pull plan holds only what is still without a body; nothing is held in memory beyond the batches in flight. A fetch error SHALL stop the pool and fail the run; a batch that fails to apply SHALL be warned about and left for the next run.

A dry run stops after Phase 1 (reporting the pull plan, downloading nothing). Progress is the two phases: `Scanning mailboxes (k/M)`, then one global `Downloading n% (done/total)` over every body.

## MODIFIED Requirements

None.

## REMOVED Requirements

### Requirement: The one-source sync runs as three account-wide phases
Replaced by the two-phase requirement above: the apply phase is folded into the hydrate pool.
