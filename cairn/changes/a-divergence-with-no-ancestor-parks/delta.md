---
cairn: delta
change: a-divergence-with-no-ancestor-parks
---

## ADDED Requirements

### Requirement: A divergence with no common ancestor parks rather than resolving itself
Two endpoints of one account holding one identity under two different bodies, with no body they ever agreed on behind them, SHALL park the divergence and SHALL NOT write either body over the other. A three-way merge SHALL NOT be attempted: with no base it could only ever park, and merging against the target's own body as the base would read the target as having changed nothing and settle on the source's, which is the overwrite parking exists to refuse.

This is the same disagreement as "Two endpoints are reconciled with each other" without the ancestor that requirement merges against, and it is what a migration and a hand-built mirror start from rather than a rare edge.

The engine records it on the shared item rather than on a binding, the two being different questions: one says an endpoint and its own server disagree, the other that the two endpoints do. The parked divergence SHALL be taken from the item, and SHALL be taken before the run pushes, marking the target being what keeps the source's body off it: a conflicted binding projects as a conflict and never as the dirty placement a round would push.

The parked placement SHALL carry the source's body as the item's own, the target's as the divergence recorded against it, and the revision the target's own pull observed, which is the shape `conflict list` and `conflict resolve` read. `--prefer-local` settles on the source's body and `--prefer-remote` on the target's, and the run after either carries the decision to both endpoints.

The item's flag SHALL be read against the same flag as the round found it. It outlives the decision that settles it, no write clearing it where the source restates the shared body, so parking on it alone would re-park what a person has already ruled on and refuse the resolution's own push on every run after.

Under `one-way` nothing parks: the source decides by declaration. A dry run stages nothing.

#### Scenario: Two endpoints holding one identity under two bodies
- GIVEN the same identity under two different bodies on two endpoints, before the store has read either
- WHEN the account is synced
- THEN the divergence is parked and counted, the run exits 2, and each endpoint still holds its own body

#### Scenario: A parked divergence settles either way round
- GIVEN a parked divergence between two endpoints that never agreed
- WHEN it is resolved with `--prefer-local` or with `--prefer-remote`
- THEN the next run carries the settled body to both endpoints and neither body was lost on the way

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
