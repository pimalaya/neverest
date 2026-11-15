---
cairn: tasks
change: a-decision-does-not-own-the-store
---

# Tasks

- [x] Read the conflicts through `PimdirReader`, which takes no lock.
- [x] Re-read per attempt and drop the handle before the merger runs.
- [x] Drive the retry path from a store written under the merger.
- [x] CHANGELOG.md.
- [x] Fold the delta into cairn/spec/sync.md and log it.
