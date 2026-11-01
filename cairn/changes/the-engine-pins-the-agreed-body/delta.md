---
cairn: change
change: the-engine-pins-the-agreed-body
---

# Delta

## ADDED Requirements

None.

## MODIFIED Requirements

### Requirement: Two endpoints are reconciled with each other
An account whose endpoints are both authoritative SHALL reconcile them with each other as well as each with its own server. An item both endpoints changed since the store last agreed with them SHALL be three-way merged against the body they both came from, SHALL resolve as an ordinary edit where the merge reports no collision, and SHALL be parked and reported where it reports one. Neither endpoint's body SHALL be written over the other's on a collision.

Neither endpoint's own reconcile can see this. Each of them agrees with its own server, and only the pair disagrees, so nothing marks it and the run reads as quiet while the two servers hold different bodies.

The store keeps the body the endpoints agreed on until every binding reconciled (pimdir SYNC §9), and records the divergence itself when the second endpoint's pull brings another body than the first's: the binding that diverged parks with the agreed body as its base, the item's body as its own and its server's as the divergence. That endpoint's own merge then settles it or reports it. neverest keeps no snapshot of its own: one read before the round and held in memory was lost to any run that stopped between the two pulls.

The endpoint whose pull landed first is the merge's left side, `ours`. That decides which body becomes the shared one and therefore what the other's divergence is recorded against; it decides nothing about a collision, which is a person's to settle through `neverest conflict resolve`.

Under `one-way` there is no divergence to reconcile: the source is the truth and the target follows it, which is what declaring an authority is for. A dry run stages nothing.

#### Scenario: Disjoint edits on two endpoints need no one
- GIVEN one card whose two endpoints each changed a field the other left alone
- WHEN the account is synced
- THEN both changes survive on both endpoints and nothing is reported as waiting

#### Scenario: A collision between two endpoints is parked, never overwritten
- GIVEN one card whose two endpoints set the same field differently
- WHEN the account is synced
- THEN the divergence is parked and counted, each endpoint still holds its own body, and rerunning changes neither

### Requirement: A divergence with no common ancestor parks rather than resolving itself
Two endpoints of one account holding one identity under two different bodies, with no body they ever agreed on behind them, SHALL park the divergence and SHALL NOT write either body over the other. A three-way merge SHALL NOT be attempted: with no base it could only ever park, and merging against the target's own body as the base would read the target as having changed nothing and settle on the source's, which is the overwrite parking exists to refuse.

This is the same disagreement as "Two endpoints are reconciled with each other" without the ancestor that requirement merges against, and it is what a migration and a hand-built mirror start from rather than a rare edge.

The store records it (pimdir SYNC §6, §9): the second endpoint's pull lands its body over an item holding another, and the binding parks with no body as its base, the item's body as its own and its server's as the divergence, before any round pushes, a conflicted binding projecting as a conflict and never as a dirty placement a round would push. That is the shape `conflict list` and `conflict resolve` read. `--prefer-local` settles on the item's body and `--prefer-remote` on the parked endpoint's, and the run after either carries the decision to both endpoints. The store clears the item's conflict on that edit, so a settled divergence does not park again.

Under `one-way` nothing parks: the source decides by declaration. A dry run stages nothing.

#### Scenario: Two endpoints holding one identity under two bodies
- GIVEN the same identity under two different bodies on two endpoints, before the store has read either
- WHEN the account is synced
- THEN the divergence is parked and counted, the run exits 2, and each endpoint still holds its own body

#### Scenario: A parked divergence settles either way round
- GIVEN a parked divergence between two endpoints that never agreed
- WHEN it is resolved with `--prefer-local` or with `--prefer-remote`
- THEN the next run carries the settled body to both endpoints and neither body was lost on the way

## REMOVED Requirements

None.
