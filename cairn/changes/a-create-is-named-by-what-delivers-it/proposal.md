---
cairn: change
id: a-create-is-named-by-what-delivers-it
status: landed
created: 2026-09-06
---

# A queued create read as a copy from the side to itself

## Why

`placement_hunks` mapped every `Created` placement to a `Copy` from the other side, the vocabulary of the two-sided relay. On an account whose sources sync alone the other side is the side itself, so a card cardamum queued was reported as `copy item … in Default from carddav to carddav`, and a move himalaya staged lost the collection it copies from.

## What

An `Add` hunk for a create with no origin on a side syncing alone, `origin` on the `Copy` hunk for a create the server copies from another collection, and the cross-side copy kept for a body the other side holds. The `--json` copy entry carries `origin` only when set.
