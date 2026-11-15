---
cairn: delta
change: stamp-the-store-at-creation
---

## ADDED Requirements

None.

## MODIFIED Requirements

### Requirement: A store written before namespaced collection ids is refused
A store written before collection ids carried their namespace SHALL NOT be read.
Neverest keeps its own state beside the store (`neverest.json`) recording the
collection-id layout, and a store directory holding a database but no such file
SHALL be refused, naming `sync --reset`. Without that guard every collection
would be looked up under a key nothing was written to and the run would report a
healthy sync over an empty replica.

Whatever materializes the database SHALL write the sidecar in the same act, so
that pair only ever describes a store an older neverest wrote. `init` SHALL
stamp the store it creates and `sync --reset` SHALL stamp the store it
recreates; a run that skipped the stamp would refuse the store it had just
created, and refuse it again after the reset the refusal asks for.

A stamp SHALL clear the derivations the sidecar records, the store it describes
having just been emptied.

#### Scenario: A fresh account syncs
- GIVEN an account whose store `init` has just created
- WHEN `sync` runs, with or without `--dry-run`
- THEN the store is read rather than refused as the unnamespaced ancestor

#### Scenario: The named remedy clears the refusal
- GIVEN a store refused for holding a database with no sidecar
- WHEN `sync --reset` runs
- THEN the recreated store carries a sidecar and the next run reads it

## REMOVED Requirements

None.
