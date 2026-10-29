---
cairn: change
id: an-events-only-calendar-declares-no-tasks
status: active
created: 2026-10-28
---

# An events-only calendar declares no tasks

## Why

Track B of unfog cairn/changes/gtd-hardening (M6, "nothing knows a calendar refuses VTODO"), following io-pimdir cairn/changes/gtd-hardening.

pimdir STORAGE §15.6 now states `x-vtodo`: an owner declaring `none` on a calendar collection says it takes no VTODO resource, the producer's gate refuses a task add or move there before it is queued, and a client reading `PimdirReader::accepts_vtodo` stops offering the calendar as a task list. io-pimdir implements the gate and the reader; nothing declares it yet. unfog over pimdir lists every `text/calendar` collection as a tasklist, and Android's `takesTasks` offers calendars whose server stores events alone (Google's CalDAV calendars among them), so a task captured there is queued, drained and only refused by the server at the push.

neverest is the owner, and a CalDAV server says what a calendar takes: `supported-calendar-component-set` (RFC 4791 §5.2.3). io-webdav already reads it into `CaldavCalendar::components` on every calendar listing; `DavClient::list_collections` drops it.

The declaration is written before the drain from the configuration alone, opening no endpoint, and again after the run; each replaces the source's whole declaration. A row learned from a listing must therefore survive the next run's first declaration, or the drain it guards runs without it.

## What

- `Collection` gains `takes_tasks: Option<bool>`, filled by the CalDAV listing from the component set (`Some(false)` when it names components but not `VTODO`, `Some(true)` when it names `VTODO`, `None` when it states no set, which RFC 4791 reads as any component); every other backend leaves it `None`.
- A local-mode run (no target) records, per source, the hub ids of the calendars its listing stated take no tasks in the sidecar (neverest.json, `taskless`), replaced at each listing, as `looked_up_roles` already is.
- `declare` (before the drain, after the run, and in `neverest drain`) adds `x-vtodo` `none` on each of those calendars the source holds, with the reason. No row anywhere else, `full` included: a collection with no row refuses nothing.

## Out

- Accounts with targets: `run_pair` records nothing, and targets are never declared.
- Google Calendar and Graph calendar sources, which keep tasks elsewhere: a source-wide `x-vtodo` `none` for them is a separate decision (their push of a VTODO is not checked here).
- `neverest check` keeps listing the Annex B capabilities alone; `x-vtodo` is a fact of the server's calendars, which a check does not hold.
- A live run: the logic is tested on a scratch store, with no server.
