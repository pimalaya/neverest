---
cairn: delta
change: a-graph-pass-states-no-guessed-removal
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: A Graph mail folder keeps one delta link over the whole folder
The pass SHALL state gone only a member one of its rows marks removed (`@removed`), where it stated gone every member the store binds or the band listed that it did not list. A Graph mail source SHALL state as vanished only what Graph states removed (pimdir SYNC §4), and SHALL NOT state gone a member a listing, a fresh delta or the pass did not list: such a member is the engine's to find absent, from the ids the pass hands, in the round's scope and under its guards.

A message a band page of a round listed and Graph dropped before the round's pass SHALL stay bound until a later round over a scope holding its date closes without listing it: its binding is stamped, so the engine never finds it absent, and a fresh delta reports no removal older than itself. Neverest does not rely on `$deltatoken=latest`, which Microsoft documents for directory resources alone.

#### Scenario: A pass that fails to list a member
- **GIVEN** a Graph folder synced under `--since 2026-06-01`, its link expired, and a fresh delta that lists neither a June message nor a July one the folder still holds
- **WHEN** it is synced under `--since 2026-07-01`
- **THEN** the round closes and both messages stay bound

#### Scenario: A message dropped between its band page and the pass
- **GIVEN** a scoped round interrupted after a band page listed a message, which Graph then drops
- **WHEN** a later run resumes the round and its pass closes it
- **THEN** the message stays bound, and a later round over its scope drops it

## REMOVED Requirements
