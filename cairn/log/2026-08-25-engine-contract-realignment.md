---
cairn: log
change: engine-contract-realignment
date: 2026-08-25
---

# Two engine contracts this crate had stopped holding, and one it never held

The engine and the store moved underneath (io-replica's `chunked-pushes` and `delete-disposition`, io-pimdir's `manual-gc`) and the working tree stopped compiling. The two compile errors were one-line adaptations. What they were standing in front of was not.

## What landed

- **A refused delete is held, not reverted** (capability `sync`). `delete-disposition` gave `ReplicaSyncOptions` a `ReplicaDeletePolicy` defaulting to `Revert`, and io-replica's own spec says a source bound to a hub SHALL be given `Keep`. Every side here is bound to the store's hub, and `sync_side` was passing the default. Reverting a tombstone states that the source still holds the member; the hub reads that as the item being alive and clears the deletion for *every* side, so a side configured `item.delete = false` (the documented recipe for a backup a remote expunge cannot lose) resurrected on both what the user had removed on one. The disposition now lives in `sync_options`, one function the run and the tests both sync through, because a wrong one is invisible until an item comes back from the dead.

- **A purge is followed by a collection** (capability `sync`). `manual-gc` took reclamation out of every write, deliberately, and the store now says so: it collects nothing by itself and leaves it to whoever owns it. Here that is this crate, and `collect_garbage` appeared nowhere, so a neverest-managed store kept every dereferenced body for ever, reported nothing about it, and passed every check it had. `sweep_retained` runs the collector after a purge that took something, and `PurgedItems` carries the objects dropped and the bytes freed beside the items removed. Only after a purge that took something: that is the moment this run knows a body was released, and the collector's cost is a walk of the whole blob tree. Orphans a crash left are `pimdir gc`'s.

  This also repairs a number the crate had been reporting wrongly: `store.purge-after` is documented as reporting the bytes it reclaimed, and until now those were bytes it had merely released.

- **`ReplicaChangeKind`** at the four push sites and the two test matches: a change is now a kind plus the idempotency key naming it.

## Verification

Both behaviour fixes went in test-first and both were checked against the old behaviour rather than only the new one. `a_side_that_may_not_delete_keeps_the_tombstone` fails under `Revert` with the placement read back `Clean` instead of `Tombstone`, which is the resurrection itself. `the_sweep_collects_the_bodies_the_purge_released` asserts the blob is gone from the tree, not merely that a count moved.

73 unit tests green, `cargo clippy --all-targets --all-features` clean, `cargo fmt`. The three live-server suites (`carddav`, `duplicates`, `relay`) are `#[ignore]`d as before and were not re-run against a server.

## Still open

The rekey blocker this crate recorded on 2026-08-25 is **fixed upstream**, in io-pimdir (`rekey-carries-the-spine`): a `Superseded` drop now licenses the rebind of the handle it names, so a `UIDVALIDITY` bump renumbers a collection instead of freezing it. `a_rekey_carries_state_by_link_id_and_bumps_the_generation_once` passes against the current working tree.

Capabilities moved: `sync`.
