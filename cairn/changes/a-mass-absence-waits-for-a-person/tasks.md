---
cairn: tasks
change: a-mass-absence-waits-for-a-person
---

# Tasks

- [x] `cli/sync`: `--accept-absences`, threaded to every source's context.
- [x] `offline/driver`: the sync options and the rekey take it; a parked round and a parked rebuild are noted per side and drained into the report.
- [x] `sync/report`: `absences`, counted as warnings and as waiting for a person; a round a park left open is not incomplete.
- [x] Tests: a round missing 24 of 25 bound messages parks and exits 2, accepted it drops them; a rebuild missing 24 of 25 parks, keeping the old handles and generation, accepted it rebuilds (failing first: the sync failed on the renumbering); the exit code of a parked absence beside its own and another open round.
- [x] Spec folded, log written, CHANGELOG, README.
