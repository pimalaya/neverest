---
cairn: delta
change: every-failure-reaches-the-result
---

# Delta

## ADDED Requirements

### Requirement: A failed pair collection is reported
A pair collection whose sync fails SHALL be recorded under `collectionPatch` as a `sync` hunk naming `source`, `target` and `collection`, with its failure, and the run SHALL exit 4. It SHALL NOT be a `scan` hunk: the failure may come from either endpoint, the store or the merge, and what the collection reported before failing stands.

#### Scenario: A pair whose target fails its listing
- GIVEN a CardDAV pair whose target answers 500 to its REPORT
- WHEN it is synced
- THEN the report holds a `sync` hunk with the error, the text does not say the account is in sync, and the run exits 4

### Requirement: A store step that fails is reported
A step of a run that fails outside any hunk SHALL be recorded under `failures`, one entry a failure: `step`, one of `queue` (the queue could not be read or drained), `bodies` (downloaded bodies could not land in the store, or a pair's full hydration failed) and `conflicts` (the waiting conflicts could not be counted); `collection`, `null` for a store-wide step; and `code`, `error` and `sources`. A `failures` entry SHALL make the run incomplete (exit 4) and SHALL be printed among the errors. A step that fails SHALL NOT read as having done nothing: rows a failed read could not classify SHALL NOT be counted as drained.

#### Scenario: A drain that fails
- GIVEN a store whose queue cannot be drained
- WHEN a sync runs
- THEN `failures` holds a `queue` entry with the error and the run exits 4

## MODIFIED Requirements

### Requirement: A drain applies the queue without syncing
A drain that parked a row SHALL exit 3, as a sync does; a store step failing SHALL fail every account of the store, which exits 4.

### Requirement: An incomplete run has its own exit code
The list of what exits 4 gains a pair collection that failed and a store step that failed.

## REMOVED Requirements
