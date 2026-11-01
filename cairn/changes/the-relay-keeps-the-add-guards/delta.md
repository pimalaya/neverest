---
cairn: change
change: the-relay-keeps-the-add-guards
---

# Delta

## ADDED Requirements

### Requirement: A relay appends only where the target's listing is closed
A message SHALL be relayed to a target only when the target has closed a round over the collection, has none open, and that round's scope holds the message's date (an undated message being in every scope), as pimdir SYNC §5 guards an add: the store then knows what the target holds there, so a message it already holds is bound to it rather than appended twice. A message held back waits for a later pass.

#### Scenario: A target still listing its mailbox
- **GIVEN** a relay account whose target's first round over the inbox is open
- **WHEN** the account is synced
- **THEN** nothing is relayed into that inbox until the round closes

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
