---
cairn: tasks
change: a-floating-time-reads-in-the-users-zone
---

# Tasks

- [x] src/zone.rs `floating_zone`, `zone_name`; tests `a_zone_without_an_iana_name_reads_as_utc` (moved) and `the_zone_is_read_from_tz` (the test binary run again under `TZ=Europe/Paris` and `TZ=America/New_York`)
- [x] Graph: anchored in the client's `floating_zone`; test `a_floating_time_is_anchored_in_the_users_zone` (Europe/Paris and UTC), failing on the UTC anchor before
- [x] Google: the fallback reads the user's zone; test `a_floating_time_is_anchored_in_the_users_zone_without_the_calendars`, failing on the UTC fallback before
- [x] `jiff/tz-system` in the `msgraph` and `gcal` features
- [x] Fold into cairn/spec/sync.md, log, CHANGELOG (the N24 line amended, both unreleased)
