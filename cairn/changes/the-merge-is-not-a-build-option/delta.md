---
cairn: delta
change: the-merge-is-not-a-build-option
---

## ADDED Requirements

None.

## MODIFIED Requirements

### Requirement: A run merges what nobody disagreed about
Unchanged in what it requires of a run. What is added is that the requirement holds of every build: the merge SHALL NOT be gated by a cargo feature of its own, and SHALL ride on the feature that decides whether a mutable-content kind exists at all. "Built in rather than configured" is a statement about build time as much as about configuration, and a feature that removes the merge makes an unconditional SHALL false in the builds that omit it.

Nothing else can reach a merge, so nothing is lost by tying them: mail is immutable-content, and a build carrying no mutable-content kind has nothing to merge rather than a merge it declines to run.

#### Scenario: A build that syncs no mutable content
- GIVEN a build made without the DAV backend
- WHEN a mail collection reconciles
- THEN no merge is reached, because mail is immutable-content, and no build option was consulted to decide it

## REMOVED Requirements

None.
