---
cairn: change
id: an-occurrence-is-found-by-its-instant
status: landed
created: 2026-11-15
---

# An occurrence is found by its instant

## Why

Ledger line N26 of the CLI alignment round of 2026-11-15, from live testing (pimalaya-52, neverest 776fca9).

- Google refused a series override in a zoned calendar: a weekly series at 10:00 `Europe/Paris` whose override names `RECURRENCE-ID;TZID=Europe/Paris:20261221T100000` failed with "Google Calendar has no occurrence 20261221T100000 of series ... to override", coded `unsupported`, the series deleted again. `Occurrences::read` dropped the `TZID`, and `names_occurrence` compared the bare wall time with the wall time of the instance's `originalStartTime` as Google wrote it, `2026-12-21T09:00:00Z` (Google writes it in the calendar's zone, here UTC), or with its UTC stamp, `20261221T090000Z`: neither matches 10:00 Paris.
- Graph patched an override against the instance as listed (`instance.to_ical()`), which always states Graph's `showAs`, `sensitivity` and `importance`: an override stating no `TRANSP`, `CLASS` or `PRIORITY` sent `"showAs":null,"sensitivity":null,"importance":null`, clearing what the series carries (found by pimalaya-52 reading, reproduced by the test below).

## What

- src/kind/event.rs: `OccurrenceId`, a `RECURRENCE-ID` or `EXDATE` value with the zone its `TZID` names, and `OriginalStart` (a date, an instant, or a wall time in no zone known); `Occurrences` carries them and the master's own document.
- src/gcal/client.rs `names_occurrence`: a date against the original date, an instant against the original start read as an instant; a wall time of no zone known (an intent's bare `recurrence_id`) against the time Google writes or the one the instance's zone shows.
- src/msgraph/client/calendar.rs `instance`: the instant compared too, beside the series-zone wall time `recurrence_id_of` gives. `instance_patch`: an occurrence Graph holds as its series has it is diffed against the document's master, the instance's own times standing in for the master's; an exception Graph holds already is diffed against itself, as before.
- src/offline/invitation.rs: `utc_stamp` and `wall_stamp` removed, nothing reading them.

## Decisions

- **Graph's held exceptions keep their diff.** Diffing every override against the master would leave on Graph an edit an override undoes (a subject an earlier override set, a moved time restored). The master-carried base, which pimalaya-52 asked for, is taken where Graph holds the occurrence as its series has it, which is the reported case and every create. io-msgraph's unreleased `MsgraphEventSeries` diffs against the base document's own exception the same way; neverest does not depend on it.
- **An `EXDATE` held on Graph is still compared by value.** A mismatch only costs a lookup finding the cancelled instance gone.
