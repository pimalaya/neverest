---
cairn: log
change: an-events-only-calendar-declares-no-tasks
date: 2026-10-29
---

# A calendar its server keeps from tasks is declared so

Every calendar a source lists now tells a task client whether it takes VTODO (pimdir STORAGE §15.6, `x-vtodo`), before the first item sync: a CalDAV calendar whose `supported-calendar-component-set` leaves VTODO out declares `none` on itself; a Google Calendar or Microsoft Graph source declares `none` source-wide (tasks live in Google Tasks and Microsoft To Do), and so does a source copied to such a target; the listing records the collections and registers the source with no checkpoint (`upsert_checkpoint` after `load_checkpoint`, never over an existing one) before any item moves, so `load_capabilities` reads the row from the start; `neverest check` reports the source-wide row and, per calendar, `takesTasks`.

Capabilities moved: sync (added A calendar its server keeps from tasks is declared so; modified Every source declares its capabilities before the drain, A drain applies the queue without syncing, A check reports what each source declares it can do).

Verified: `cargo test` green, fmt and clippy clean; the new tests fail on 474753d's code (shims named in tasks.md where a seam did not exist).
