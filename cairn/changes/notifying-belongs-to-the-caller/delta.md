---
cairn: delta
change: notifying-belongs-to-the-caller
---

## ADDED Requirements

None.

## MODIFIED Requirements

### Requirement: Entering a conflict is said once
A run SHALL warn once for each placement that entered conflict during it, and SHALL say nothing about one an earlier run already parked. Neverest SHALL raise no desktop notification of its own, and SHALL NOT link a notification daemon.

The report SHALL keep the two apart, and this is what makes notifying possible without building it in: the conflicts a run marked are listed item by item, and the count the store holds waiting is carried beside them. A caller reading the JSON report notifies on entry by testing the first, once, with no state of its own to keep, and can name the item, its collection and its side while doing so.

An unattended tool that repeats itself is one a user silences. A five-minute schedule and one unresolved conflict is otherwise nearly three hundred notifications a day, all naming the same card.

The exit code SHALL NOT be read as that signal. It answers a wider question, whether the run left anything waiting at all, which a parked conflict, a refused duplicate `UID` and a rejected write all satisfy.

#### Scenario: The second run is quiet
- GIVEN a conflict marked by one run and left unresolved
- WHEN a later run observes it again
- THEN it is not warned about again, and the report lists it as outstanding rather than as newly marked

#### Scenario: A caller notifies on entry
- GIVEN a run that marked one conflict and a store holding three others from earlier runs
- WHEN the JSON report is read
- THEN the newly marked one is listed and the outstanding count is four, so a caller announces one item rather than four

## REMOVED Requirements

None.
