---
cairn: delta
change: a-relay-fails-one-message-at-a-time
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: A relay appends only where the target's listing is closed
Adds: a relayed body SHALL be exactly the size its listing stated, or its append SHALL NOT complete; a message whose relay fails SHALL be reported under `rejected` (action `append`) and the relay SHALL go on with the next message, each failed side recovered first; only a side that cannot be recovered SHALL fail the collection.

## REMOVED Requirements
