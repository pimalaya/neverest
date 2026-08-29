---
cairn: change
change: a-refused-write-is-not-an-applied-hunk
---

# Delta

## ADDED Requirements

### Requirement: A refused write is reported, never counted as applied
A write a remote would not take SHALL be reported, naming the source, the
collection, the item, what was attempted and why it failed, in the text and
`--json` reports alike. It SHALL NOT be counted among the hunks the run applied:
the hunk the run derived for that item SHALL be taken back, the item patch being
the plan and not the outcome. It SHALL keep appearing on every run until the
write lands or the reason is removed.

A body that never reached the wire, because the blob tree no longer holds it,
SHALL be reported the same way and for the same reason: the change is still in
the store and the next run will try again.

A create refused with the no-uid-conflict precondition SHALL keep its own entry
and gain no second one, since that entry names the identity and the remedy, and
one write is one line.

#### Scenario: A server refuses a body
- GIVEN a source that answers an update with a refusal
- WHEN the run reports
- THEN the refusal is named with its reason, the update is not among the hunks applied, and a run with no other work does not read as having written anything

## MODIFIED Requirements

### Requirement: A conflicted run succeeds, with its own exit code
A run that reconciled its collections and left work behind SHALL exit with a code
distinct from both success and failure, and SHALL report the outstanding conflict
count read from the store rather than the count the run itself marked.

Three states are that code, and they are one class: a divergence waiting for a
decision, a duplicate `UID` a side refuses, and a write a side would not take.
Each leaves something the store holds and could not deliver, each is re-reported
on every run until a person acts, and a rerun on its own changes none of them.

A conflict is one item wide, and so is a refusal. Failing the run would stop
every other item over one divergence, and under a supervisor restarting on
failure it would loop over a state no supervisor can resolve. The distinct code
says the same thing without pretending the run broke.

The two conflict counts differ and the difference matters: the engine emits
nothing for a placement already parked, which is what keeps notifications quiet
across repeated runs, and which is also why the run's own tally is not the number
of decisions waiting.

#### Scenario: A parked conflict does not fail the run
- GIVEN a collection holding one parked conflict beside ordinary items
- WHEN it is synced
- THEN the ordinary items reconcile, the run exits with the conflict code, and the outstanding count is reported

#### Scenario: A run that could not deliver a write says so
- GIVEN a source that refuses the only write a run had to make
- WHEN the run ends
- THEN it exits with the same code, rather than reporting success over a change that stayed in the store
