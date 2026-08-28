---
cairn: change
change: one-warning-per-parked-action
---

# Delta

## MODIFIED Requirements

### Requirement: Neverest is the store's sole owner and drains the queue first
Neverest SHALL be the only process writing a pimdir store; frontends read it and
enqueue mutations through io-pimdir's producer queue. At the start of every sync
run, before any network work, each collection with pending queue work SHALL be
drained (`drain_collection`: exactly-once apply-and-delete per action,
permanently bad actions parked, transient failures left queued in order). The
applied counts SHALL be logged (info when nonzero) and reported.

Every parked action SHALL surface in the run report until repaired, and SHALL
surface **once per run**. A parked row belongs to the store rather than to a
source, the queue recording none, so reading them where the drain runs reports
each of them once per source that ran: one row read as three problems on an
account syncing mail, contacts and calendar. They SHALL therefore be read once,
after every source has drained, in a dry run as much as in a real one.

The subsequent sync of a drained collection pushes the resulting dirty state. An
action kind the drain cannot apply itself (a capability-bound intent such as
`submit`) SHALL be left pending for the phase that can, never parked.

#### Scenario: One parked row on a three-source account
- GIVEN an account whose mail, contacts and calendar sources all drain
- WHEN one queue action is parked
- THEN the run reports one warning, not one per source
