---
cairn: change
id: engine-contract-realignment
status: landed
created: 2026-08-25
---

# Two engine contracts neverest had stopped holding, and one it never held

## Why

The engine and the store moved under this crate (io-replica's `chunked-pushes` and `delete-disposition`, io-pimdir's `manual-gc`), and the working tree stopped compiling: `ReplicaChange` became a kind plus an idempotency key, and `PimdirPurgeReport` lost its `bytes`. Both are one-line adaptations. What they were hiding is not.

**A refused delete now reverts by default, and neverest is hub-backed.** `delete-disposition` gave `ReplicaSyncOptions` a `ReplicaDeletePolicy`, defaulting to `Revert`, and its own spec says a source bound to a hub SHALL be given `Keep`, with the reason: reverting a tombstone says "this source still holds the member", which the hub reads as the item being alive, so it clears the deletion for *every* side and mirrors the item back to the one it was deleted on. `sync_side` passes `..Default::default()`, so a side configured `item.delete = false` (the documented recipe for a backup a remote expunge cannot lose) resurrects on both what the user removed on one. That is the opposite of what the setting is for, and it arrived silently with the default.

**Nothing collects garbage.** `manual-gc` removed reclamation from every write, deliberately: an object at refcount zero is unreferenced rather than deleted, because the batch that attaches a body may not be the one that indexed it. The store therefore never collects itself and says so, leaving it to whoever owns the store. Here that is this crate, and it does not: `collect_garbage` appears nowhere. A neverest-managed store keeps every dereferenced body for ever, reports nothing about it, and passes every check it has. Meanwhile `store.purge-after` is documented as reporting "the items and bytes it reclaimed" and the bytes it reported were the ones it had merely released.

## What

- `ReplicaChangeKind` at the four push sites and the two test matches.
- `sync_options`, one function building the options both `sync_side` and the tests sync under, carrying `ReplicaDeletePolicy::Keep`. Written once because a wrong disposition is invisible until an item comes back from the dead.
- `sweep_retained` runs the collector after a purge that took something, and `PurgedItems` carries what it dropped and freed beside what the purge removed. Only after a purge, since that is the moment this run knows a body was released and the collector's cost is a walk of the whole blob tree; orphans a crash left are `pimdir gc`'s.
