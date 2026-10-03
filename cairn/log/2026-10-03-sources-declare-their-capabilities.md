---
cairn: log
change: sources-declare-their-capabilities
date: 2026-10-03
---

# Sources declare their capabilities

pimdir draft-03 lets producers refuse an unsupported action before it is queued; neverest, the owner, now declares what each source can do, on io-pimdir 0.6.0.

## What landed

- src/offline/capability.rs: a declaration per backend (IMAP, Gmail, Graph mail, CardDAV, Google People, Graph contacts, CalDAV, Google Calendar, Graph calendars) from configuration and rights alone; driver.rs declares every source before the drain.
- src/offline/driver.rs, submit.rs: a `submit` sent only by the source it names; the copy it asks for filed once sent, by `replace_action` and a second drain, unless the provider files sent mail itself.
- src/gcal/client.rs: `sendUpdates` from `SCHEDULE-AGENT` on update, from the attendees on delete.
- Tests: the declaration's unit tests; end to end against Stalwart and Radicale with himalaya, cardamum and calendula (pimdir cairn/changes/capabilities/e2e.md).

## Capabilities moved

- sync: four requirements added (declaration, named sender, sent copy, Google notifications).

Open follow-ups stay in cairn/changes/archive/sources-declare-their-capabilities/tasks.md: collection rows from access rights, a Google event notifying on creation, CalDAV scheduling detection, online meetings, native reply and cancel, modified instances, the misplaced right accepted silently; iMIP is deferred to the NLnet 2027 plan.
