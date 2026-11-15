---
cairn: log
change: a-floating-time-reads-in-the-users-zone
date: 2026-11-15
---

# A floating time reads in the user's zone

Ledger line N25 of the CLI alignment round of 2026-11-15.

## What landed

- src/zone.rs: `floating_zone` and `zone_name`, out of src/offline/index.rs, which reads them from there with src/cli/index.rs.
- src/msgraph/client.rs, src/msgraph/client/calendar.rs: `GraphClient::floating_zone`, the zone `anchor` writes a floating time in, a `TZID` (a `Z` for UTC).
- src/gcal/client.rs: `GcalClient::floating_zone`, the fallback of a calendar stating no zone.
- Cargo.toml: `jiff/tz-system` in the `msgraph` and `gcal` features.
- Tests: `a_floating_time_is_anchored_in_the_users_zone` (Graph) and `a_floating_time_is_anchored_in_the_users_zone_without_the_calendars` (Google), each failing on the UTC anchor before; `the_zone_is_read_from_tz` runs the test binary again under two `TZ` values, also in a build with `gcal` alone.

## Capabilities moved

- [sync](../spec/sync.md): *A floating time is anchored before Google or Graph takes it* amended.
