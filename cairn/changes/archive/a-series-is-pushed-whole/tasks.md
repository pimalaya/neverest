---
cairn: tasks
change: a-series-is-pushed-whole
---

# Tasks

- [x] src/kind/event.rs: `Occurrences::read`, `refuse_ranges`
- [x] src/msgraph/client/calendar.rs: `write_occurrences`, `instance`, `cancel_instance`; `add_event` deletes the series again on failure; `update_event` writes them after the master
- [x] src/gcal/client.rs: `write_overrides`, `instance`; `add_item_stream` deletes the series again on failure; `update_item_stream` writes them after the master
- [x] src/offline/capability.rs: `calendar.occurrence.update` `partial` on `gcal` and `msgraph-calendar`
- [x] Tests: `a_series_lands_with_its_excluded_and_its_moved_occurrence`, `a_series_whose_override_finds_no_occurrence_is_taken_back` (Graph, scripted server), `a_series_lands_with_its_moved_occurrence`, `a_series_whose_override_finds_no_occurrence_is_taken_back` (Google, scripted session), each failing on the master-only push; `a_series_sets_its_exclusion_and_its_override_apart`, `an_override_of_every_later_occurrence_is_refused`
- [x] Fold into cairn/spec/sync.md, log, CHANGELOG
- [ ] An override or an `EXDATE` dropped by an update, which the server keeps (open)
