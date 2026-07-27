---
cairn: change
id: retention-sweep
status: landed
created: 2026-08-07
---

# Sweep the store's retained items on a schedule

## Why

A pimdir store no longer deletes an item. When its last source binding vanishes
(a remote expunge, propagated) the row is **retained**: hidden from the sync
seam and from normal listings, but kept, body and all. That is the storage's
choice, not the engine's, and it is unconditional (io-pimdir workstream A).

Retention without reclamation is a leak: a mailbox that churns would grow
without bound. Somebody has to purge, and purging is a *schedule*, not a
semantic, so it belongs to a client rather than to the store. Neverest is the
natural sweeper on desktop: it already holds the store lock for the whole run,
it is the store's sole owner, and it runs on the cadence the user chose.

The same mechanism is what makes a **backup** honest. A read-only remote side
plus no purge delay means a remote expunge retires the local row without ever
losing the item: the copy stays restorable, which is what the removed m2dir
`soft-delete` flag used to promise and could not deliver once local sides were
dropped.

## What

- `store.purge-after`, a human duration (`"90d"`, `"12h"`, `"0"`) parsed into a
  `HumanDuration` newtype that round-trips through the document. **Unset means
  never purge**; `"0"` purges immediately, reproducing the old terminal delete.
  No boolean: the delay subsumes the on/off switch, so there is one knob and no
  way to spell a contradiction.
- The sweep runs **after** the sync, before the report is finalised, on both
  run paths (two-side and single-source), never in a dry run. Running it after
  means an item this run retired starts its delay now rather than being
  reclaimed by the very run that retired it.
- It **warns rather than fails**, like the send channel: a store that cannot be
  swept is a housekeeping problem, not a reason to fail a run that synced.
- `sync --no-purge` skips it for one run.
- The report gains a `purged` section (items and bytes reclaimed), rendered in
  the text output and in `--json`, so a run says what it freed.

The cutoff is computed by neverest (`now - purge-after`, RFC 3339, millisecond
precision) and handed to `purge_retained_before`, matching the shape the store
stamps `retained_at` with. io-pimdir stays clock-free.
