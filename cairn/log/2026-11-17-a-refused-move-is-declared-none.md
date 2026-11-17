---
cairn: log
change: a-refused-move-is-declared-none
date: 2026-11-17
---

# A refused move is declared none

Finding NEV-18 of the CLI family analysis of 2026-11-15: `calendar.item.move` and `contacts.card.move` were declared `full` on Google Calendar and on Graph contacts and calendars while their connectors refuse a relocation, so the gate passed a frontend's move, the push was rejected (exit 3), the destination uploaded a copy (on Graph a new event, its attendees invited again) and a later run deleted the original.

## What landed

- src/sync/capability.rs: both rows `none` on those three backends, with the reason.
- Test `a_move_the_connector_refuses_is_declared_none`.
- Trivial fix: no proposal.

## Capabilities moved

- [sync](../spec/sync.md): *Every source declares its capabilities before the drain* amended.
