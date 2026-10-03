---
cairn: log
change: a-move-is-delivered-once
date: 2026-10-03
---

# A move is delivered once

A move staged through the store landed twice in its target on IMAP. Each half of a move delivers alone (pimdir SYNC §5), the second standing down once it reads what the first did; but phase 1 scanned and pushed collections over several connections, and two overlapping pushes both read the target's create as pending, the target uploading the message while the source relocated it with `MOVE`.

## What landed

- src/offline/driver.rs: a `pushing` lock in `phase1_spine`, held across a collection's push passes, so one collection of a source pushes at a time; scans, conflict resolution and the hydration phase stay parallel.
- tests/moves.rs (ignored, Stalwart :143): a message bounced eight times between two mailboxes, held once on the server and in the store after each run. It failed on bounce 1 or 2 in each of three runs without the fix, and passed in five runs with it.

## Capabilities moved

- sync: "A source's collections push one at a time".

pimdir's "in either order" reads as one push after the other; stating that pushes of one source never overlap is a candidate clarification for a later draft.
