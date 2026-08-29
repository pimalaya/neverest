---
cairn: change
change: a-decision-does-not-own-the-store
---

# Delta

## ADDED Requirements

### Requirement: Deciding never owns the store
A conflict command SHALL read the store through a handle that owns nothing and
takes no lock (pimdir SPEC §8), and SHALL NOT hold the store's owner lock across
a decision. A resolution SHALL re-read the divergence and its bodies for each
attempt, release the store before the merger runs, and take the store again, under
the run lock, only to apply what came back.

The store's owner lock lives on the handle, so a handle kept for a command's
lifetime refuses every sync of that store for as long as a person sits in an
editor. That is the window the staleness guard exists for, and holding the lock
across it makes the guard unreachable: the only thing that moves a placement's
conflict revision is a sync of that store, so the revision cannot move, and the
refusal, the retry and the re-export never run.

#### Scenario: A sync writes the store while the merger is up
- GIVEN an interactive resolution whose merger is still running
- WHEN a sync of that store records a newer conflict revision
- THEN the write is not refused, and the decision the merger returns is exported again against what arrived
