---
cairn: delta
change: endpoints-reconcile-with-each-other
---

## ADDED Requirements

### Requirement: Two endpoints are reconciled with each other
An account whose endpoints are both authoritative SHALL reconcile them with each other as well as each with its own server. An item both endpoints changed since the store last agreed with them SHALL be three-way merged against the body they both came from, SHALL resolve as an ordinary edit where the merge reports no collision, and SHALL be parked and reported where it reports one. Neither endpoint's body SHALL be written over the other's on a collision.

Neither endpoint's own reconcile can see this. Each of them agrees with its own server, and only the pair disagrees, so nothing marks it and the run reads as quiet while the two servers hold different bodies.

The ancestor SHALL be read before the round pulls. Recording a remote content change drops the stale body from the shared item and from that source's base together, so once both endpoints have pulled, the store holds their two new bodies and nothing they both came from, and a merge attempted after that has no base to merge against.

The source is the merge's left side, `ours`. That decides which body becomes the shared one and therefore what the target's divergence is recorded against; it decides nothing about a collision, which is a person's to settle through `neverest conflict resolve`.

Under `one-way` there is no divergence to reconcile: the source is the truth and the target follows it, which is what declaring an authority is for. A dry run stages nothing.

#### Scenario: Disjoint edits on two endpoints need no one
- GIVEN one card whose two endpoints each changed a field the other left alone
- WHEN the account is synced
- THEN both changes survive on both endpoints and nothing is reported as waiting

#### Scenario: A collision between two endpoints is parked, never overwritten
- GIVEN one card whose two endpoints set the same field differently
- WHEN the account is synced
- THEN the divergence is parked and counted, each endpoint still holds its own body, and rerunning changes neither

### Requirement: One identity is one item across an account's endpoints
An identity two endpoints of one account already hold SHALL bind to a single shared item, whichever endpoint the store reads first, and SHALL NOT be minted a second key. The minting rule answers one collection holding one identity twice; a sibling endpoint holding it once is not that.

Identity is settled by the fetch that reads it, against the placements the store answers with. A source's projection carries the copies its sibling holds and it does not, so that the merge can derive the append, and reading those as claims on the identity turns the second endpoint's own card into a duplicate of the first endpoint's. The store SHALL therefore be read, where identity is settled, as what that source holds and nothing else.

A mirror and a migration both start from two servers already holding the same items, so binding only when an item propagates from one side is binding in exactly the case that does not matter.

#### Scenario: Two servers already holding one card
- GIVEN the same card on both endpoints before the store has read either
- WHEN the account is synced
- THEN the store holds one item under the card's own key, neither endpoint is asked to take a copy it already holds, and a second run is quiescent

## MODIFIED Requirements

### Requirement: A duplicated identity is mirrored, not reported
One collection holding two resources under one identity SHALL be mirrored as two items, and SHALL produce no report entry of its own. The store holds what the source holds, and a report entry is for work a run could not do.

Two resources means two resources of one source. An identity a sibling endpoint of the same account also holds is one item bound twice, not two items: see "One identity is one item across an account's endpoints".

## REMOVED Requirements

None.
