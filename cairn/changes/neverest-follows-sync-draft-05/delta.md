---
cairn: change
change: neverest-follows-sync-draft-05
---

# Delta

## ADDED Requirements

### Requirement: An edit names the body it was made over
A conflict resolution, a merge and a one-way overwrite SHALL state the body they were made over (the queued `update`'s `if_match`, the `Edit` mutation's, pimdir STORAGE §15.3, SYNC §7): an edit another write replaced since SHALL park rather than drop that write unmerged. A body staged with its action SHALL be staged in one step with it, so a refused action leaves no body in the tree.

### Requirement: A change over an unreadable body is withheld and reported
A change whose body is missing from the store or does not hash to its name SHALL be withheld, the placement staying as it was (pimdir STORAGE §7, SYNC §5), and the run SHALL report it under `unreadable` with the side, the collection and the count, and SHALL exit 2: the store needs a repair a rerun does not make.

### Requirement: A body a policy overruled is kept and reported
A body the configured policy did not adopt, kept by the store as a retained item (pimdir SYNC §9), SHALL be reported under `overruled` with the side, the collection, the item's handle and the retained item's key. It SHALL be a warning and SHALL NOT change the exit code: the policy decided as configured.

### Requirement: A stated identity is named without its escape
A stated identity the store filed with a leading `esc:` (pimdir STORAGE §9), because it begins like a derived key, SHALL be named to a server without it, alone or inside a minted key; a derived key SHALL name none.

## MODIFIED Requirements

### Requirement: A body reaches the blob tree under a lock
A body neverest writes SHALL go through the write side io-pimdir hands a lock holder: a fetched body streams into the owner's stage (`PimdirStore::blob_stage`), which hashes it as it passes, and a settled or merged conflict body is staged by the producer that enqueues it, in one step with its action (`PimdirProducer::enqueue_with_body`). neverest SHALL NOT hash or write a body itself.

## REMOVED Requirements

None.
