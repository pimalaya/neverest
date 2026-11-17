---
cairn: log
change: a-target-sends-nothing-and-narrows
date: 2026-11-17
---

# A target sends nothing and narrows what it takes

Finding NEV-7, its `targets.<name>.smtp` and `targets.<name>.<backend>.collection.filter` parts, ruled by the owner on 2026-11-17.

## What landed

- A target's `smtp` table is refused at load, naming the target: an account sends through a source (its `smtp`, else its native send), so the table loaded, `account check` signed in to it, and no sync ever used it.
- A queued send naming one of the account's targets as its sender parks, reported under `submitted`, where it stayed pending for ever.
- A target's `collection.filter` narrows the pair: a collection it leaves out is neither created on the target nor synced, applied to both sides as the source's filter is, so one source can feed different collections to different targets. It was loaded and ignored.

## Spec

sync.md: the `smtp` requirement, the collection filter requirement.
