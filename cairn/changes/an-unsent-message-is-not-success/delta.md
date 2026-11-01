---
cairn: change
change: an-unsent-message-is-not-success
---

# Delta

## ADDED Requirements

None.

## MODIFIED Requirements

### Requirement: A conflicted run succeeds, with its own exit code
A run that reconciled its collections and left work behind SHALL exit with a code distinct from both success and failure, and SHALL report the outstanding conflict count read from the store rather than the count the run itself marked.

These states are that code, and they are one class: a divergence waiting for a decision, a duplicate `UID` a side refuses, a write a side would not take, a send or an intent this run parked (refused for good, or uncertain), and a queued action the store holds parked. Each leaves something a person has to settle, and a rerun on its own changes none of them. Conflicts, refusals and parked queue actions are re-reported on every run until a person acts; a parked send or intent is reported by the run that parked it.

A conflict is one item wide, and so is a refusal. Failing the run would stop every other item over one divergence, and under a supervisor restarting on failure it would loop over a state no supervisor can resolve. The distinct code says the same thing without pretending the run broke.

The two conflict counts differ and the difference matters: the engine emits nothing for a placement already parked, which is what keeps notifications quiet across repeated runs, and which is also why the run's own tally is not the number of decisions waiting.

#### Scenario: A parked conflict does not fail the run
- GIVEN a collection holding one parked conflict beside ordinary items
- WHEN it is synced
- THEN the ordinary items reconcile, the run exits with the conflict code, and the outstanding count is reported

#### Scenario: A run that could not deliver a write says so
- GIVEN a source that refuses the only write a run had to make
- WHEN the run ends
- THEN it exits with the same code, rather than reporting success over a change that stayed in the store

#### Scenario: A refused send is not a success
- GIVEN a queued send the server refuses for good
- WHEN the run ends
- THEN the send is parked and the run exits with the conflict code

### Requirement: An incomplete run has its own exit code
A run that could not do all its work SHALL exit with a code of its own, distinct from success, failure and the conflict code: a source it could not scan, an endpoint or a send channel it could not open, a hunk it could not apply, a send or an intent that failed without parking, a source that gave up throttled, a round left open on a collection it synced, or convergence passes that ended still moving. It SHALL win over the conflict code, the report still counting what waits for a person.

An unreachable server is the common case. The run stops nothing else over it, so it is no failure, yet a run with nothing to do and a run that could not reach its server both exited 0, and a caller told offline from idle by parsing error strings. A rerun clears this state on its own, which is what sets it apart from the conflict code: that one waits for a person, this one for the network.

A send or an intent that parked is not this: it waits for a person, and its row says why.

#### Scenario: An unreachable source is not an idle run
- GIVEN an account whose only source refuses the connection
- WHEN it is synced
- THEN the scan error is reported and the run exits with the incomplete code

## REMOVED Requirements

None.
