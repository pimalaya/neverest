---
cairn: log
change: a-floating-time-is-anchored
date: 2026-11-15
---

# A floating time is anchored

Ledger line N24 of the CLI alignment round of 2026-11-15, from live testing (calendula part 2, P2-F3).

## What landed

- src/kind/event.rs: `has_floating`, `anchor_floating`.
- src/gcal/client.rs: a floating time anchored in the calendar list entry's `timeZone`, UTC with a warning when none.
- src/msgraph/client/calendar.rs: a floating time anchored in UTC, with a warning, Graph stating no zone for a calendar.
- An anchored write reports no revision, so the next sync reads it back.

## Capabilities moved

- [sync](../spec/sync.md): *A floating time is anchored before Google or Graph takes it* added.
