---
cairn: log
change: an-occurrence-is-found-by-its-instant
date: 2026-11-15
---

# An occurrence is found by its instant

Ledger line N26 of the CLI alignment round of 2026-11-15, from live testing (pimalaya-52).

## What landed

- src/kind/event.rs: `OccurrenceId` (value and `TZID`), `OriginalStart`, `Occurrences::master`.
- src/gcal/client.rs: `names_occurrence` compares original starts as instants; a series at 10:00 `Europe/Paris` finds the instance Google lists at `2026-12-21T09:00:00Z`.
- src/msgraph/client/calendar.rs: `instance` compares the instant too; `instance_patch` diffs an occurrence Graph holds as its series has it against the document's master, so an override stating no `TRANSP`, `CLASS` or `PRIORITY` no longer nulls `showAs`, `sensitivity` and `importance`.
- src/offline/invitation.rs: `utc_stamp`, `wall_stamp` removed.
- Tests: `a_zoned_override_names_the_instance_google_lists_in_utc` and `an_override_patches_what_it_changes_against_its_series`, each failing on 776fca9 as reported; `an_occurrence_starts_at_the_instant_its_zone_reads`.

## Left

- An `EXDATE` Graph already holds is matched by value, a mismatch costing one lookup.
- Graph's patch still nulls `showAs`, `sensitivity` or `importance` when the master states the property and the override drops it, which reads as the override clearing it.

## Capabilities moved

- [sync](../spec/sync.md): *A series is pushed whole or not at all* amended.
