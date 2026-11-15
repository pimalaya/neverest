---
cairn: change
id: a-floating-time-is-anchored
status: landed
created: 2026-11-15
---

# A floating time is anchored

## Why

Live testing of 2026-11-15 (calendula part 2, P2-F3; ledger line N24 of cli-alignment-2026-11-15, medium): an event calendula queued at a floating time on a calendar Google or Graph syncs was rejected on every sync (`Google needs a DTSTART carrying either a UTC Z suffix or a TZID`, Graph the same), a dead end the producer could not see, the rejected row's `id` empty.

## What

- Before a create or an update, the Google and Graph calendar sources anchor every floating `DTSTART`, `DTEND`, `RECURRENCE-ID`, `EXDATE` and `RDATE` of the document's events (src/kind/event.rs `has_floating`, `anchor_floating`).
- Google anchors in the calendar's zone, read from its calendar list entry (`timeZone`), as a `TZID`; UTC with a warning when the calendar states none.
- Graph states no zone for a calendar: it anchors in UTC, with a warning naming the write.
- An anchored write reports no revision, so the next sync reads the event back and the store holds what the server does.
- The empty `id` is a-refused-create-is-named-by-its-link-id.

## Decisions

- **UTC on Graph, as the ledger line asks.** A Graph calendar resource carries no zone; the user's zone is `mailboxSettings.timeZone`, which needs `MailboxSettings.Read`, a scope the sample does not ask for, and comes as a Windows name. The zone the store's index already reads floating times in (`TZ`, then the system's, src/offline/index.rs `floating_zone`) would keep the time a person sees; left to the owner.
- **The store follows the server.** Reporting no revision is what an online-meeting request already does: the next sync reads the anchored event back.
