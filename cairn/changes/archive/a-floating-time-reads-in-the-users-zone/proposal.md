---
cairn: change
id: a-floating-time-reads-in-the-users-zone
status: landed
created: 2026-11-15
---

# A floating time reads in the user's zone

## Why

Ledger line N25 of the CLI alignment round of 2026-11-15, from N24 (a-floating-time-is-anchored). A floating time pushed to Graph was anchored in UTC, Graph stating no zone for a calendar, so a floating 10:00 landed at 11:00 Paris time; a Google calendar stating no zone did the same.

## What

- src/zone.rs: `floating_zone`, the user's zone (`TZ`, then the system's, UTC with a warning when it has no IANA name), moved out of src/offline/index.rs so the Google Calendar and Graph sources read the zone the search index states.
- src/msgraph/client/calendar.rs: a floating time anchored in that zone, as a `TZID` (a `Z` when it is UTC), with a warning naming the write and the zone.
- src/gcal/client.rs: a calendar stating no zone falls back to the same zone, where it fell back to UTC.
- Cargo.toml: `msgraph` and `gcal` turn on `jiff/tz-system`, without which a build with neither `search` reads no system zone.

## Decisions

- **The zone is read once per run, at the client's opening.** `GcalClient` and `GraphClient` hold it, so a test names one; the index refresh reads the same `OnceLock`.
- **Graph's `mailboxSettings.timeZone` stays unread**, as N24 left it: it needs `MailboxSettings.Read` and comes as a Windows name.
