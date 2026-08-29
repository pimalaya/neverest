---
cairn: change
change: a-run-names-what-it-parked
---

# Delta

## ADDED Requirements

### Requirement: A collection's report reaches the account's whole
A report filled while reconciling one collection SHALL be merged into the
account's report entire. Every arm a collection fills SHALL travel: the item and
collection patches, the divergences it parked, the duplicates a side refused, the
writes a side would not take and the collisions it skipped. Only what a
collection never had an opinion about SHALL stay behind, namely the account's own
name and dry-run flag, the retention sweep, which runs once for the account, and
the outstanding conflict count, which is read from the store rather than summed.

Collections are reconciled across a worker pool and their reports merged at a
barrier, so this is the one place the account's report is assembled. A merge that
carries the patch alone leaves the run saying how many items it touched and never
what it left behind: no warning block, and no notification, the announcement
being raised from the parked list and returning early when it is empty.

#### Scenario: A conflict parked in a worker reaches the printed report
- GIVEN a run whose worker parks a divergence while reconciling a collection
- WHEN the account's report is printed
- THEN the warnings block names the item, its collection and its source, and the notification is raised
