---
cairn: tasks
change: report-pulled-changes
---

- [x] `flag_snapshot` (handle → flags) before the pull.
- [x] `itemize_pulled`: FlagsChanged → precise add/remove hunks (diffed vs
      snapshot); Vanished → delete hunk; Added skipped (already a Fetch).
- [x] Wired into `mailbox_spine` (snapshot before pull, itemize after).
- [x] Build, fmt, clippy clean; 15 tests pass.
- [x] Live: a remote `\Flagged` add reported as `add [\flagged] to message N`
      (only the newly-added flag, not the already-present `\Seen`).
- [ ] Fold delta into `cairn/spec/sync.md`; write log entry.
