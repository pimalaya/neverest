---
cairn: tasks
change: an-occurrence-is-found-by-its-instant
---

# Tasks

- [x] src/kind/event.rs `OccurrenceId`, `OriginalStart`, `Occurrences::master`; tests `a_series_sets_its_exclusion_and_its_override_apart` (zones, master) and `an_occurrence_starts_at_the_instant_its_zone_reads`
- [x] Google: `names_occurrence` by instant; test `a_zoned_override_names_the_instance_google_lists_in_utc` (a Europe/Paris series whose instance Google lists at 09:00Z), failing before with the live refusal; `an_instance_is_the_occurrence_its_original_start_names` extended
- [x] Graph: `instance` by instant too; `instance_patch` against the series; test `an_override_patches_what_it_changes_against_its_series` (a listed instance with importance high, sensitivity private, showAs free; an override renaming it), failing before on `"showAs":null,"sensitivity":null,"importance":null`
- [x] The same wall-time comparison checked elsewhere: Graph's `instance` matched the series-zone wall time (right when the `TZID` is the series' zone, as live), now also the instant; Google takes `EXDATE`s in the recurrence it imports, matching none; the invitation intents reach the same lookups
- [x] Fold into cairn/spec/sync.md, log, CHANGELOG (the series line amended, unreleased)
