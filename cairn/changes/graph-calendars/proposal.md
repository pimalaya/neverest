---
cairn: change
id: graph-calendars
status: landed
created: 2026-10-01
---

# Graph syncs calendars

## Why

`a-graph-source-declares-its-domain` left Graph without calendars, io-msgraph having no events resource. io-msgraph now has calendars, events and an `ical` projection, so a Microsoft 365 account can sync its calendars as it syncs its mail and contacts, which MOA's Microsoft 365 agendas wait for.

## What

A third `GraphKind`, `Calendar`, behind an `msgraph-calendar` key. Calendars are collections; a series and its exceptions are one item. The enumeration lists each calendar's events in full every run rather than through the calendar view delta, whose time window would delete every event that leaves it on the other sources.

## Not in scope

Pushing an exception edited locally; seeing an occurrence edited on Graph before its master's `changeKey` moves; an end-to-end run against a tenant.
