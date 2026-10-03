---
cairn: log
change: calendar-invitation-intents
date: 2026-10-03
---

# Calendar invitation intents

Graph and Google calendar sources now perform the `calendar-reply` and `calendar-cancel` intents of pimdir Annex B.2, and a new Google meeting created through the store invites its attendees without losing its UID.

## What landed

- src/offline/invitation.rs: the intents read from the queue and their `v: 1` payload; failures classified the way `submit` does.
- src/offline/driver.rs: `drain_invitations` beside `drain_submits`, the item located through the performing source's binding; the report's `intents`; the declaration re-run after the sync.
- src/msgraph/client/calendar.rs: the `accept`, `tentativelyAccept`, `decline` and `cancel` actions.
- src/gcal/client.rs: the reply as a patch of the account's attendee, the cancel as a delete, both `sendUpdates=all`; a scheduled event the account organises inserted with its `iCalUID`.
- src/offline/capability.rs: both intents declared per held calendar (Graph full, Google reply full and cancel partial), `none` source-wide; CalDAV `none` with the RFC 6638 route named.
- Tests: unit tests; live on Google (a meeting invited to, refused a reply, cancelled; an invitation answered) and Graph (a meeting refused a reply and cancelled; a mailed invitation accepted).

## Capabilities moved

- sync: one requirement added (calendar intents), one modified (Google notifications on creation).

Open follow-ups stay in cairn/changes/archive/calendar-invitation-intents/tasks.md: CalDAV through the server's scheduling, the Exchange global object id of mailed invitations, modified instances.
