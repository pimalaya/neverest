---
cairn: delta
change: engine-contract-realignment
---

## ADDED Requirements

### Requirement: A refused delete is held, never reverted
Every side SHALL sync under `ReplicaDeletePolicy::Keep`. Both refusals (`push` off, or `item.delete = false`) run through that one disposition, and each side here is bound to the store's hub, which fixes the answer: reverting a tombstone states that the source still holds the member, and a hub reads that as the item being alive (add-beats-delete across sources), so it clears the deletion for every side and mirrors the item back to the one it was deleted on.

A side configured to take no deletes would then resurrect on both what the user removed on one, which is the opposite of what that setting is for.

#### Scenario: A read-only side keeps the removal
- GIVEN a staged delete on a side whose `item.delete` is false
- WHEN the side is synced
- THEN nothing is pushed and the tombstone stays, rather than being undone into a clean row

### Requirement: A purge is followed by a collection
The store reclaims nothing by itself (pimdir SPEC §5), so the retention sweep SHALL run the collector after a purge that removed rows, and SHALL report the objects it dropped and the bytes it freed beside the items the purge removed. A purge releases a body; it does not reclaim one.

The collector SHALL NOT run after a sweep that took nothing: its cost is a walk of the whole blob tree, and a purge that removed no row released nothing. Orphan blobs a crash left are not this run's to find.

#### Scenario: The bytes a purge reports are bytes that left
- GIVEN a retained item past the purge cutoff, holding a body nothing else references
- WHEN the sweep runs
- THEN the item is purged, the object row is dropped, and the blob is gone from the tree

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
