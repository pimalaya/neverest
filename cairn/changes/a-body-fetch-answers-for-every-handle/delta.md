---
cairn: change
change: a-body-fetch-answers-for-every-handle
---

# Delta

## ADDED Requirements

### Requirement: A body fetch answers for every handle it was asked about
A batched body fetch SHALL be treated as complete only when it answers for every
handle it carried. A batch answering for fewer SHALL fall back to a per-item
fetch for the remainder, exactly as a batch that errors already does, and SHALL
report the shortfall.

A backend that answers for a subset is not a backend that failed, so nothing
surfaces as an error; but the engine cannot tell an unanswered handle from an
unasked one, so an unanswered handle is recorded nowhere and re-requested on
every later run. That is a run that fetches a whole collection, stores nothing
and reports itself in sync.

#### Scenario: A server answers for two cards out of sixty-four
- GIVEN a batched fetch of 64 handles that returns 2 bodies
- WHEN the run continues
- THEN the other 62 are fetched one by one and the shortfall is reported

### Requirement: An empty body is refused, never stored
A body of zero bytes SHALL fail the fetch, naming the item and its collection.
No kind neverest syncs has an empty body: a message carries headers and a card
carries at least its `BEGIN` and `END` lines.

An empty body stored is worse than a fetch that fails. Its link id is the digest
of nothing, so every empty body a server returns resolves to the same identity;
the second one collides with the first, the duplicate-link-id floor freezes it,
and the collection stays frozen for every later run.

#### Scenario: A server returns zero-length cards
- GIVEN a server answering card bodies with zero bytes
- WHEN the run fetches them
- THEN it fails naming the first such card, rather than storing an item whose identity is the digest of nothing
