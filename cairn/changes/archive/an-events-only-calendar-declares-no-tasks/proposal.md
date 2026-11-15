---
cairn: change
id: an-events-only-calendar-declares-no-tasks
status: landed
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
- Google Calendar and Graph calendar sources declare `x-vtodo` `none` source-wide from the configuration: their providers keep tasks apart (Google Tasks, Microsoft To Do), and neither API stores a VTODO.
- An account with targets records, under the source, what both endpoints of each pair state (a task in the hub crosses to the target), and a Google or Graph calendar target makes the source's row source-wide. Targets stay undeclared.
- A run declares again right after a listing is recorded, so a calendar met mid-run is gated from its first item sync rather than from the end of the run.
- Before that declaration, the listing records the source as syncing each collection it is about to sync and holds no state in: a `sources` row with no checkpoint, through the canonical `load_checkpoint` then `upsert_checkpoint` (pimdir checks/invariants.sh reads such a row as syncing, "by its checkpoint alone"), never overwriting a checkpoint, as Android's `EventStore.ensureSynced` does. The store counts a source as syncing a collection from that row (io-pimdir `load_capabilities`), so a calendar listed but not synced yet (`sync --declare-only`, or a run that has not reached it) refuses a task right after the listing.
- `neverest check` lists the source-wide row among the capabilities and a CalDAV calendar's statement as `takesTasks` on the collection.

## Out

- Declaring a target: a target with an `x-vtodo` row alone would read as a declared source supporting nothing.
- A live run: the logic is tested on a scratch store, with no server.
