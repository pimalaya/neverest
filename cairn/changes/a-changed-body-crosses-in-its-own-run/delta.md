---
cairn: delta
change: a-changed-body-crosses-in-its-own-run
---

## ADDED Requirements

### Requirement: A change one endpoint made crosses in the run that observed it
An item an account's two endpoints both hold, whose body exactly one of them changed, SHALL be delivered to the other in the run that observed the change. The changed body SHALL be hydrated before the pushing passes, so the run that reports the update is the run that makes it.

A run SHALL NOT report a write it has not attempted. An update itemized from the projection and left unsent by a hydration that never happened is such a write, and it reads as applied on every run while the two endpoints stay divergent.

Under a declared authority there is no divergence to reconcile, so an item both endpoints changed SHALL hydrate the deciding side's body and overwrite the other in that same run. Under shared authority the same item is a divergence, and the conflict path merges or parks it instead.

The far side's own permission decides what may be hydrated for it: `item.create` for a copy it does not hold, `item.update` for a body it does.

#### Scenario: A mirror delivers an edit in one run
- GIVEN one source and one target holding the same item
- WHEN the item's body changes on the source and the account is synced once
- THEN the target holds the new body, the update is reported once, and the next run reports nothing

#### Scenario: A one-way account overwrites in the run that saw the difference
- GIVEN a `one-way` account whose source and target hold one item under two bodies
- WHEN the account is synced once
- THEN the target carries the source's body, nothing is counted as a conflict, and the run exits 0

## MODIFIED Requirements

## REMOVED Requirements
