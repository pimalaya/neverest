---
cairn: tasks
change: dry-run-pull-plan
---

# Tasks

- [x] `sync/hunk.rs`: `EmailHunk::Fetch { side, mailbox, id }` + Display.
- [x] `driver`: `itemize_fetches` (not-yet-Full, non-tombstone items) run in
      `sync_mailbox_single` for both dry and real runs, before the dry-run return.
- [x] Verify on live Stalwart: fresh 1-source `--dry-run` lists the fetches and
      downloads nothing; real run reports + downloads; already-synced = nothing.
- [x] Build/test/fmt/clippy; relay unregressed.
- [x] Fold `delta.md` into `cairn/spec/sync.md`; log; land.
