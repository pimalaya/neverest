---
cairn: log
change: graph-mail-folders-at-every-depth
date: 2026-11-17
---

# Graph mail folders at every depth

Finding NEV-17 of the CLI family analysis of 2026-11-15, ruled by the owner on 2026-11-17: every level.

## What landed

- Graph mail folders are listed at every depth, each folder with children walked in turn, where the listing stopped at two levels: `Inbox/Projects/2026` and its mail were absent from the store, the run reporting success, while himalaya over Graph walks every level. The spec's two sentences (two levels deep, against a listing naming every collection it holds) now agree.
- A `/` in a folder's own display name is written `%2F` in its collection id, and a `%` `%25`: a top-level `A/B` and the child `B` of `A` got one id and one was dropped. The collection's name is the path with the escapes undone.
- Collection ids of folders whose display name holds `/` or `%` change; no released neverest holds such a store (v1 stores start empty).

## Spec

sync.md, the Graph mail paragraph.
