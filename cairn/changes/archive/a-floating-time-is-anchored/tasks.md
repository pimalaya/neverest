---
cairn: tasks
change: a-floating-time-is-anchored
---

# Tasks

- [x] src/kind/event.rs: `has_floating`, `anchor_floating`; tests `a_floating_time_is_anchored_in_the_zone_given`, `a_floating_time_is_anchored_in_utc_without_a_zone`, `a_date_a_zone_or_utc_is_no_floating_time`
- [x] src/gcal/client.rs `anchor` (the calendar list entry's zone); test `a_floating_time_is_anchored_in_the_calendars_zone`, failing on the refusal before
- [x] src/msgraph/client/calendar.rs `anchor` (UTC); test `a_floating_time_is_anchored_in_utc`, failing on the refusal before
- [x] Fold into cairn/spec/sync.md, log, CHANGELOG
