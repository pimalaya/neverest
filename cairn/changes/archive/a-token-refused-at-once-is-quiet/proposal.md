---
cairn: change
id: a-token-refused-at-once-is-quiet
status: landed
created: 2026-11-15
---

# A token refused at once is quiet

## Why

Live testing of 2026-11-15 (ledger line N16 of pimalaya/.github cairn/changes/cli-alignment-2026-11-15): Google told the public "Holidays in France" calendar its sync token had expired on the second pass of every run, seconds after the first pass issued it, so its 209 events were listed again and a warning printed every run; the other calendars kept their tokens. The ledger asked whether the incremental request differs from the one that issued the token, Google requiring the same parameters.

## What

It does not: src/gcal/client.rs `list` sends one `GcalEventsListParams` for both, `maxResults` and `showDeleted=true`, no `singleEvents`, `timeMin` or `orderBy`, the token alone added. The refusal is Google's way with that calendar. The token handling stays (a full listing again), and the log is quieter:

- `GcalClient` remembers the tokens its listings ended on this run; one refused that it issued is said at debug ("refused as soon as issued"), one an earlier run issued at warn, as before.
- A test pins the request: the resumed listing's request is the issuing one plus `syncToken`, and the listing after a 410 is the issuing one again.

## Decisions

- **No "no token here" memory**: remembering across runs that a calendar refuses tokens would save nothing (a full listing is what a refused token costs anyway) and would stop a calendar whose refusals end from ever resuming.
- **One connection a run**: Google Calendar's connection budget is one, so the tokens a client remembers are the run's.
