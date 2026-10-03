---
cairn: change
id: calendar-invitation-intents
status: landed
created: 2026-10-03
---

# Calendar invitation intents

Self-contained: a session with no prior context can take it from here.

## Why

pimdir Annex B.2 defines `calendar-reply` and `calendar-cancel`, and every calendar source declared them `none` ("neverest does not perform it yet"). A frontend (calendula, MOA) answering an invitation or cancelling a meeting had to call Graph or Google itself, outside the store, which is what the store exists to avoid. Google also imported every new event to keep its UID, so a meeting created through the store invited nobody.

## What

- Perform the queued `calendar-reply` and `calendar-cancel` intents at the start of each sync, beside `submit` and on its model: performed even one-way, acknowledged once performed, a permanent failure parked with its error, a transient one left pending, each attempt reported (`intents` in the JSON report).
- Graph: the event's own actions, `accept`, `tentativelyAccept`, `decline` (`sendResponse: true`) and `cancel`, the comment carried.
- Google: the account's own attendee answered (`responseStatus`, `comment`) by a patch guarded by the etag, `sendUpdates=all`; a meeting the account organises deleted with `sendUpdates=all`, Google's notice carrying no comment.
- Both declared on each calendar the source syncs, `none` source-wide (pimdir STORAGE §15.6 Implementations), the rows re-declared after the run so a calendar met for the first time takes them.
- CalDAV: no verb; both stay `none`, the detail pointing at the write the server schedules from (RFC 6638).
- Google: a new scheduled event the account organises is inserted with its UID as `iCalUID` and `sendUpdates=all`, which keeps the UID and invites; any other is still imported.
