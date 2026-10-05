---
cairn: change
change: graph-bodies-in-json-batches
---

# Delta

## ADDED Requirements

### Requirement: Graph mail bodies are fetched in JSON batches
A Graph mail source SHALL fetch the `Full` tier of an id set through JSON batches (`POST /$batch`) of at most 20 raw MIME gets (`/messages/{id}/$value`), each body answered base64-encoded and decoded before it reaches the blob store, matched to its message by request id whatever order Graph answers in. A get Graph throttles (429 or a common 5xx), alone or with its whole batch, SHALL be sent again after the longest `Retry-After` stated, a bounded number of times; a get refused otherwise, or still throttled, SHALL be left unanswered so the per-item fallback fetches it and surfaces its error.

#### Scenario: Twenty bodies, one round trip
- **GIVEN** a Graph mail folder holding twenty messages whose bodies the store lacks
- **WHEN** a sync hydrates them
- **THEN** one batch request fetches all twenty, stored byte for byte as a direct get would

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
