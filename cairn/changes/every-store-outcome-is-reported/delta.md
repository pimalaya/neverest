---
cairn: change
change: every-store-outcome-is-reported
---

# Delta

## ADDED Requirements

### Requirement: A push no server answered is pushed again
A push the connector gave no outcome for SHALL stay pending (pimdir SYNC §5), and the run SHALL report the count the last pass of each collection left under `unanswered`, with the side and the collection, and SHALL exit 3: the next run pushes them again.

### Requirement: A rejected create is reported once
A create the engine reports its server rejected (`rejected_creates`) SHALL be reported under `rejected`: with the reason the connector recorded, or under `refused` for a duplicate `UID`, else as a `create` whose server gave no reason. It SHALL be reported once whichever records it.

### Requirement: A resolution names what the push supports in part
`conflict resolve` SHALL stage its body in one step with the gate (`enqueue_with_body_checked`) and SHALL print the capabilities a source supports in part that the next push relies on, under `partial` in its JSON.

## MODIFIED Requirements

### Requirement: A body a policy overruled is kept and reported
A body a policy did not adopt, kept by the store as a retained item (pimdir SYNC §9), SHALL be reported under `overruled` with the collection and the retained item's key, and either the side and the item's handle on it, for a body a side's write or a mutation settled, or the queue row (`action`), for one the drain kept aside applying a queued action, in a sync and in `drain`. It SHALL be a warning and SHALL NOT change the exit code: the policy decided as configured.

### Requirement: A store error carries its code
A store failure that ends a command SHALL carry, in its JSON failure object, the code io-pimdir gives it (`PimdirError::code`), spelled the same: `store-newer`, `store-older`, `store-uncreated`, `store-foreign`, `store-corrupt`, `store-not-current`, `store-stale`, `store-hash-algo`, `store-busy`, `store-owned`, `store-staging`, `store-in-flight`, `unsupported`, `index-fts5-missing`. A failure a caller cannot act on (an SQL or I/O error, a malformed action) SHALL carry none, nor SHALL a code neverest cannot meet (`index-busy`, which `index` answers as `busy`; `index-missing`, `index-outdated` and `index-query-only`, a query client's; `cutoff-invalid`, neverest writing its cutoff as the store stamps). No message SHALL advise deleting the store: the remedies are an upgrade, `sync --reset` on a store holding no pending work, and `sync --reset --force`, which moves it aside.

#### Scenario: A downgrade under `--json`
- **GIVEN** a store written by a newer neverest
- **WHEN** an older neverest runs `--json drain` on the account
- **THEN** it exits 1 printing a failure object whose `code` is `store-newer`

#### Scenario: A store root naming another program's database
- **GIVEN** a pimdir.db holding another program's tables
- **WHEN** neverest runs `--json drain` on the account
- **THEN** it exits 1 printing a failure object whose `code` is `store-foreign`, the database untouched

### Requirement: An incomplete run has its own exit code
A run that could not do all its work SHALL exit with a code of its own, distinct from success, failure and the conflict code: a source it could not scan, an endpoint or a send channel it could not open, a hunk it could not apply, a push its server answered nothing for, a send or an intent that failed without parking, a source that gave up throttled, a round left open on a collection it synced (but one its listing parked over a mass absence), or convergence passes that ended still moving. It SHALL win over the conflict code, the report still counting what waits for a person.

An unreachable server is the common case. The run stops nothing else over it, so it is no failure, yet a run with nothing to do and a run that could not reach its server both exited 0, and a caller told offline from idle by parsing error strings. A rerun clears this state on its own, which is what sets it apart from the conflict code: that one waits for a person, this one for the network.

A send or an intent that parked is not this: it waits for a person, and its row says why. Nor is a round a mass absence parked: a rerun meets the same listing.

#### Scenario: An unreachable source is not an idle run
- GIVEN an account whose only source refuses the connection
- WHEN it is synced
- THEN the scan error is reported and the run exits with the incomplete code

## REMOVED Requirements

None.
