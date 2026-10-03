---
cairn: change
id: sources-declare-their-capabilities
status: landed
created: 2026-10-03
---

# Sources declare their capabilities

Self-contained: a session with no prior context can take it from here.

## Why

pimdir STORAGE §15.6 and Annex B (change `capabilities` in the pimdir repository, draft-03 pending) let a producer refuse an action before it is queued, instead of finding out from a rejected push. The owner declares what each source can do; neverest is the owner. Without a declaration every source is undeclared and nothing is gated, so himalaya, cardamum, calendula and MOA cannot tell the user that Graph mail cannot move, that Google People has one address book, or that a CalDAV server notifies nobody.

## What

- Every run, before the drain, neverest declares each source it runs from its backend and its configured rights alone, before any credential is read (src/offline/capability.rs, `declaration`).
- Mail (IMAP, Gmail, Graph), contacts (CardDAV, Google People, Graph contacts) and calendars (CalDAV, Google Calendar, Graph calendars) declare every Annex B capability of their kind, `none` with its reason. JMAP stays undeclared.
- A right the configuration withholds is `none`; a one-way source refuses every write and keeps its intents.
- A `submit` is sent only by the source its payload names (`source`).
- Google Calendar honours `SCHEDULE-AGENT`: an update notifies the attendees (`sendUpdates=all`) only when the resource is scheduled, a delete when the event has attendees.

What is declared `none` or `partial` today and why:

| Backend | Capability | Support | Reason |
| --- | --- | --- | --- |
| CalDAV | `calendar.scheduling` | partial | the server's own scheduling (RFC 6638), when it has one |
| CalDAV | `calendar.online-meeting` | none | CalDAV has no online meeting |
| Google Calendar | `calendar.scheduling` | partial | a new event is imported to keep its UID, which notifies nobody |
| Google Calendar, Graph | `calendar.occurrence.update` | none | a modified instance does not push yet |
| Graph calendars | `calendar.scheduling` | partial | Graph notifies every attendee, `SCHEDULE-AGENT` aside |
| every calendar | `calendar.reply`, `calendar.cancel` | none | neverest does not perform them yet |
| Google Calendar, Graph | `calendar.online-meeting` | none | neverest does not perform it yet |
| Google People | `contacts.card.move`, `.copy` | none | a single address book |

## Non-goals

Collection rows from the remote's access rights, performing calendar intents and online meetings: listed in tasks.md as follow-ups.
