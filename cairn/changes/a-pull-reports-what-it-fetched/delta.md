---
cairn: change
change: a-pull-reports-what-it-fetched
---

# Delta

## ADDED Requirements

### Requirement: A run reports the bodies it pulls, whatever the tier
A run SHALL report the bodies it fetches, and SHALL report the same ones whether
or not it is a dry run. The report SHALL NOT depend on the tier a kind resolves
its identity at.

The pull plan is the placements carrying no body yet, so it SHALL be read before
the probe that resolves link ids. A kind with no cheap `Meta` tier resolves its
link id from the body, so the probe hydrates it; a plan read afterwards is empty
for exactly the items the run is about to pull, and the run calls itself
quiescent having downloaded a collection.

#### Scenario: A first contacts sync says what it did
- GIVEN an empty store and an address book holding one card
- WHEN `sync` runs without `--dry-run`
- THEN it reports fetching that card, as `--dry-run` said it would, rather than reporting itself already in sync
