---
cairn: delta
change: an-undated-search-may-be-refused
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: A provider narrows a scope with a margin
IMAP finds the messages with no `Date` header by a search of their own, and a server refusing it does not fail the round: the round lists its dated messages, warns once, and the connection asks no more. Scenario *A server refusing the undated search* added.

## REMOVED Requirements
