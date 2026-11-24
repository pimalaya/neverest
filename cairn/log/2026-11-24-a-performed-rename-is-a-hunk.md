---
cairn: log
change: the-live-night-findings
landed: 2026-11-24
---

# A performed rename is a collection hunk

Task N13 of the-live-night-findings. On the live night of 2026-11-23, a `collection-rename` a frontend queued was performed, the run listing it among the intents alone and ending "already in sync", though it had renamed a folder on the server.

`drain_collection_intents` (src/sync/driver/queue.rs) now reports a performed rename as a new `CollectionHunk::Rename` (JSON `kind: "rename"`, `collection`, `to`). Before, `collectionPatch` was `[]` beside `intents: [{kind: "collection-rename", ...}]`. After:

```
Intents (1):
 - collection-rename #2 performed by carddav

Collection patches (1):
 - rename collection book to Again on carddav

Account home synchronized: 1 hunks
```

tests/fakedav answers `PROPPATCH` of the book's display name for it. Capability moved: sync.md, "The report accounts for every write the run made".

The night also saw "not synchronized" after a performed rename; not reproduced, it needs that run's other errors.

## Checked

- tests/collection_intents.rs `a_performed_rename_is_a_collection_hunk`: without the hunk it fails (`collectionPatch: []`, `left: 0 right: 1`), with it it passes, in the JSON and the text.
