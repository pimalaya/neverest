---
cairn: change
change: a-drain-opens-no-endpoint
---

# Delta

## ADDED Requirements

### Requirement: A drain applies the queue without syncing
`neverest drain` SHALL apply the account's queued actions as a sync's drain does, after declaring the sources from the configuration, reading no credential and opening no endpoint. It SHALL NOT wait for the store: while a run holds `sync.lock` it SHALL report `busy` and exit 3, the queue untouched. It SHALL report each row it applied, with the `seq` of the item an `add` created, the rows it parked, and how many it left for a sync.

#### Scenario: An event created offline
- **GIVEN** a calendar whose server is unreachable, and an event calendula queued there
- **WHEN** `neverest drain` runs
- **THEN** the event is in the store under the `seq` the drain reports, and the next sync pushes it under that same `seq`

## MODIFIED Requirements

### Requirement: Neverest is the store's sole owner and drains the queue first
Neverest SHALL be the only process writing a pimdir store; frontends read it and enqueue mutations through io-pimdir's producer queue. A sync run SHALL drain the store's queue once, at its start, after declaring the sources and before reading any credential or opening any endpoint, so a run that cannot reach its servers still applies what was queued: exactly-once apply-and-delete per action, permanently bad actions parked, transient failures left queued in order. The store applies each action as the source syncing its collection (pimdir STORAGE §15.2), so the drain is the store's and not a source's. `--declare-only` SHALL NOT drain; a dry run drains its replica. The applied counts SHALL be logged (info when nonzero) and reported.

Every parked action SHALL surface in the run report until repaired, and SHALL surface **once per run**, read after every source has run, in a dry run as much as in a real one.

The subsequent sync of a drained collection pushes the resulting dirty state. An action kind the drain cannot apply itself (a capability-bound intent such as `submit`) SHALL be left pending for the phase that can, never parked.

#### Scenario: A sync that cannot connect
- **GIVEN** an event queued on a calendar whose server is unreachable
- **WHEN** the account syncs and exits 3
- **THEN** the event is in the store, with its `seq`

## REMOVED Requirements

### Requirement: A source drains the collections of its own namespace
The drain is the store's, applied as the source syncing each collection (pimdir STORAGE §15.2), and runs once per run before any source: no source drains, so none needs narrowing.
