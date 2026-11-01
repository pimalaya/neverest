---
cairn: change
change: neverest-builds-the-search-index
---

# Delta

## ADDED Requirements

### Requirement: The store's owner builds its search index
A build with the `search` feature (on by default) SHALL refresh the store's search index (index.db, pimdir SEARCH §3, §4) after a sync, last, for at most the account's `store.index-timeout` (60 seconds unless set, `"0"` turning it off), and after a drain, a run performing the queued intents alone and a run declaring collections, for at most 2 seconds. A dry run SHALL NOT refresh. A refresh cut by its deadline SHALL leave the index behind the store and the next refresh SHALL go on from there.

The run SHALL report the refresh under `index`: the changes folded, the bodies newly indexed, the stamps still `behind`, whether another indexer held it (`busy`), and the error of a failed one. An index failure SHALL be a warning and SHALL NOT fail the run or change its exit code: the index is derived from the store.

`neverest index` SHALL build the index until it holds every change the store has, reading no credential and opening no endpoint, and SHALL exit 3 answering `busy` while another indexer holds it.

A reset SHALL take the index with the store: dropped with an idle store, moved aside with it by `--force`.

#### Scenario: A message a sync landed is found by a frontend
- **GIVEN** a sync landing a message whose subject says "Quarterly report"
- **WHEN** a frontend opens the index for queries alone and searches `quarterly`
- **THEN** it finds the message

#### Scenario: A first build longer than the deadline
- **GIVEN** a store whose index a refresh's deadline cut
- **WHEN** the next run refreshes it
- **THEN** it goes on from where the cut left it until the index is current

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
