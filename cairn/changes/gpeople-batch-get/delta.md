---
cairn: delta
change: gpeople-batch-get
---

# Delta

## ADDED Requirements

### Requirement: Google People bodies are read in batches
A `gpeople` source SHALL read the persons a page or a hydration asks for through `people:batchGet`, up to 200 a request, never one `people.get` a person. A person the batch answers not found SHALL be gone, never an error, and each person read SHALL be stored with its etag as its batch lands.

#### Scenario: An address book of 450 contacts
- **GIVEN** a `gpeople` source whose 450 contacts the store does not hold
- **WHEN** it is synced
- **THEN** their bodies are read in 3 requests

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
