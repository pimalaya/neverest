---
cairn: log
change: connector-listings-fail-rather-than-shrink
date: 2026-11-19
---

# An open Graph series older than its window says so

The owner's ruling of 2026-11-19 (option 3) on the open question of connector-listings-fail-rather-than-shrink: an open-ended Graph series stays read within five years of today either side, and one starting before that floor is reported rather than dropped silently. The createdDateTime floor ruled earlier the same day was voided by its live check.

## What landed

- src/msgraph/client/calendar.rs: `series_windows` says whether an open series starts before its floor; `series` answers that floor, and `calendar_item` (bodies batched or read alone, now given their calendar) notes it as (collection, id, floor). An update's read of the series notes nothing.
- src/msgraph/client.rs: `SeriesClips`, shared by a source's connections as `SizeHints` is, held on `MsgraphAccount` (src/account.rs) and set on each connection by src/client.rs `open`; `Account::clipped` collects them by endpoint.
- src/sync/report.rs, src/sync/driver.rs: the report's `clipped` rows, `{side, collection, id, since}`, `since` the floor in RFC 3339 UTC, printed among the warnings; no exit code. No existing row named an item read in part (`coverage` is a collection's scope, `unlisted` a count of members a capped listing left out), hence a key of its own.
- The body is still written to the other endpoints: the window's exceptions are complete, and failing the read would fail every yearless birthday in every run.
- Tests: `an_open_series_older_than_its_window_is_noted_read_from_its_floor` (a 1604 series noted from today minus five years, one born a year ago noted nowhere), failing on the code before with the sharing in place and no note; `a_clipped_series_is_a_warning_and_changes_no_exit_code`; `series_windows_span_the_range_within_what_graph_allows` checks the flag.

Left: an exception further than five years ahead is left out the same way and reported by no row, every open series having its ceiling there.

## Spec

- [sync](../spec/sync.md): new *An open Graph series older than its window says so*; the Graph calendars paragraph points at it.
