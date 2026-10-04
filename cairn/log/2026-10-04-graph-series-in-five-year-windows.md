---
cairn: log
change: graph-series-in-five-year-windows
date: 2026-10-04
---

# Graph series in five-year windows

Every calendar of the test mailbox synced, Birthdays and the holidays calendar among them, ended on exit 3: Graph refuses an instances listing over more than five years (400, "The range between the start and end dates is greater than the allowed range. Maximum number of years: 5"), and the window of an open-ended series ran five years and a day past its start. A bounded series over more than five years failed the same way.

## What landed

- src/msgraph/client/calendar.rs: `series_windows` splits a series' range into windows of five years less a day; an open-ended series is read within five years either side of today, so a birthday from 1604, Outlook's year for one without a year, costs three requests rather than dozens.
- tests/msgraph.rs: `every_graph_calendar_of_the_mailbox_syncs`, live, now exits 0; `a_graph_teams_meeting_carries_its_join_link`, live, skipped on the test tenant, which has no Teams.

## Capabilities moved

- sync: the `msgraph-calendar` series window.
