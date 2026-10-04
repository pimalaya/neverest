---
cairn: log
change: calendar-stamps-do-not-collide
date: 2026-10-04
---

# Calendar stamps do not collide

MOA saw a Graph meeting edited right after its creation come back as a conflict, and a later cancellation create the meeting again. Graph moves an event's `changeKey` and `LAST-MODIFIED` on its own when it sends the invitations, so the next run found both sides changed and three-way merged them. The local edit rewrote `DTSTAMP` and raised `SEQUENCE`, the Graph projection restamped `DTSTAMP` and carries no `SEQUENCE`, and the ical-rs merge counted both as collisions: the item parked, its edit never pushed. The cancellation then deleted the meeting on Graph, and the edit still pending locally recreated it.

## What landed

- src/kind/merge.rs: the calendar merge drops `DTSTAMP`, `LAST-MODIFIED`, `CREATED` and `SEQUENCE` from the base and the remote side before merging, the local side keeping its own, and raises each merged component's `SEQUENCE` to the remote's where it counted further.

## Capabilities moved

- sync: a run merges what nobody disagreed about (calendar stamps).
