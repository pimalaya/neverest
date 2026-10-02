---
cairn: log
change: graph-calendars
landed: 2026-10-01
---

# Graph syncs calendars

`msgraph-calendar` syncs a Microsoft 365 account's calendars: a third `GraphKind`, its half in src/msgraph/client/calendar.rs, over io-msgraph's new calendars and events resources and its `ical` projection.

The enumeration lists each calendar's lone events and series masters in full every run, the master's `changeKey` as revision. Graph's only event delta runs over a calendar view, a time window, and an event leaving a sliding window would read as deleted and be deleted on every other source, so the delta is not used; the cost is one listing per calendar per run. A series is one item, its exceptions read from the instances of its own range, capped at five years past an open-ended start, its cancelled occurrences, which Graph returns only `$select`ed, as EXDATEs.

The UID rides the projection's stash extended property, as a Graph contact's does, and a create whose UID did not survive is deleted again and refused. Updates and deletes are checked against the master's `changeKey` first, Graph having no conditional write. Only the master is written back, and an occurrence edited on Graph alone is seen once the master's `changeKey` moves, which no offline check can confirm Graph does.

Not done: exception push, an end-to-end run against a tenant. io-msgraph and ical-rs are unreleased, so Cargo.toml patches them to their local checkouts.

Capabilities moved: **sync** (Microsoft Graph is a first-class source, sources are remote backends only, the sugar keys, modified).
