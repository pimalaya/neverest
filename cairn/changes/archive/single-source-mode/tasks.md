---
cairn: tasks
change: single-source-mode
---

# Tasks

- [x] `config`: `left`/`right` → `Option<SideConfig>`; `StoreConfig { root }` +
      `AccountConfig.store`; `sides()` helper + `SideName`.
- [x] `init`/`check`: iterate configured sides, require ≥1; `store_dir` root
      override.
- [x] `driver::run` dispatches `run_single` / `run_dual`.
- [x] `run_single` + `sync_mailbox_single` (pull → itemize → push → settle) +
      `hydrate_all` (retain every body) + `itemize_single`.
- [x] Wizard literals (`discover.rs`, `edit.rs`) updated for `Option` + `store`.
- [x] Build/test/fmt; end-to-end 1-source verify (sync, Himalaya read, flag
      write-back propagates, report shows the push).
- [x] Fold `delta.md` into `cairn/spec/sync.md`; add `cairn/log`; land.
