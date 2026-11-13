---
cairn: delta
change: neverest-follows-draft-10
---

# Delta

## ADDED Requirements

### Requirement: The owner's files live under local/
Neverest SHALL keep every file of its own that follows a store under the store's local/ (pimdir STORAGE §3, §6): neverest.json, the sync.lock and the `submitting` mark, and no file of its own beside the store's. It SHALL create local/ `0700` and each file in it `0600`, never through a link or a file under the name. A dry run's replica SHALL carry local/.

### Requirement: A remove relocates or deletes, and says which
A `Remove` carrying a target SHALL relocate its member and answer `relocated`. When the target already holds the member's identity it SHALL delete the member instead, on the change's precondition, and answer no relocation (pimdir SYNC §4, §5). The target is asked only for an identity its backend can search for, and never under a minted key.

### Requirement: An add its server does not name is accepted with no handle
An append the server takes without naming the member SHALL be answered accepted with no assigned handle (pimdir SYNC §4), never rejected, and SHALL NOT be appended again. The relay SHALL still fail on such an append.

### Requirement: A create left waiting is reported
The report SHALL list under `waiting` the creates the last pass of each collection left waiting, each with its `side`, `collection`, key as `id` and `reason` (`out-of-scope`, `delivered`, `undeliverable`, `other`). It is no warning and changes no exit code.

### Requirement: A member named apart from its create is reported
The report SHALL list under `disagreed` every member a listing named apart from a create of its own side holding its identity (pimdir SYNC §5, §6), once, as a warning.

### Requirement: A create naming no member is owed no body fetch
Neverest SHALL NOT plan a body fetch under a provisional handle, and a reset SHALL count a delivered create among the work it refuses over without `--force`.

### Requirement: An intent stating an unknown version stays pending
An intent whose payload states an integer `v` other than the one neverest reads SHALL be left pending and untouched (pimdir STORAGE §15.3). One stating no `v`, or one that is no integer, SHALL park as malformed.

## MODIFIED Requirements

### Requirement: An account holds named sources
The header cites pimdir storage-draft-10, sync-draft-10 and search-draft-10 (io-pimdir 0.11, pimdir ae8b231). Neverest keeps its own state under the store's local/.

### Requirement: A store the format outgrew is refused with its remedy
Its scenario names a store of `storage-draft-09`, the one this move strands.

### Requirement: A run holds the store lock, waiting bounded
The sync.lock is under local/ of the actual store directory.

### Requirement: A send that may have been delivered is never sent again
Such a `submit` SHALL be parked as an intent whose outcome is unknown (pimdir STORAGE §13, §15.2, io-pimdir `unconfirm_action`), its error beginning with the reserved prefix `unconfirmed: `, which the store writes, and reported in `submitted` with `unconfirmed: true`. The `submitting` mark is under the store's local/.

### Requirement: The sidecar is replaced whole
The temporary file is beside the sidecar, under the store's local/.

### Requirement: The sidecar holds each account
neverest.json is under the store's local/.

### Requirement: A store laid out by an earlier neverest is refused
A store directory holding a sidecar beside its database, where an earlier neverest kept it, SHALL be refused as `store-layout` too.

### Requirement: A store error carries its code
`lock-replaced` is relayed. `scope-invalid`, `store-reset`, `action-not-intent` and `backup-occupied` are codes neverest cannot meet.

## REMOVED Requirements
