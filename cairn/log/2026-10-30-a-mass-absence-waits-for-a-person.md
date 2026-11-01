---
cairn: log
change: a-mass-absence-waits-for-a-person
date: 2026-10-30
---

# A mass absence waits for a person

io-pimdir parks a round or a rekey whose listing leaves out more than half of at least 20 bound items (pimdir SYNC §5, §8); neverest could neither confirm such absences nor report them.

## What landed

- src/cli/sync.rs: `sync --accept-absences`, for the run only; threaded through `driver::run` to every source's context (`SourceCtx::accept_absences`).
- src/offline/driver.rs: the sync options and `PimdirRekey::accept_absences` take it; `Noted` keeps per side the rounds and rebuilds that parked, `itemize_noted` drains them into the report. A parked rebuild ends the collection's sync for the run (`sync_side_rebuilding`), where the sync listed it again and failed on the renumbering; `rebuild_collection` no longer reads a generation a parked rekey never wrote.
- src/sync/report.rs: `absences` (`side`, `collection`, `absent`, `bound`, `rebuild`), a warning naming `--accept-absences`; `left_waiting` counts it (exit 2), and `incomplete` leaves out the round a park left open.
- README.md, the `sync` help: what exit 2 covers and what `--accept-absences` confirms.
- Tests: `a_round_missing_most_of_the_inbox_parks_until_accepted`, `a_rebuild_missing_most_of_the_inbox_parks_until_accepted` (over the fake IMAP server), `a_parked_absence_waits_for_a_person` (exit code and JSON).

## Exit code

2, not 3: the listing that parked is what the server answers, so a rerun alone parks again; a person confirms the absences or finds why the server answered short. Exit 3 is for a run a rerun completes on its own, and still wins over 2 for any other open round.

## Evidence

Without the parked-rebuild branch, `a_rebuild_missing_most_of_the_inbox_parks_until_accepted` failed with `Sync imap imap/INBOX ... The handles of INBOX were renumbered, it is rekeyed first`: the sync listed the collection again after a rekey that wrote nothing.

## Capabilities moved

- sync: a listing missing most of a collection waits for a person (added); a conflicted run succeeds, with its own exit code (modified: a parked absence, an unreadable body); an incomplete run has its own exit code (modified: not a round a mass absence parked).
