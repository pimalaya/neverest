---
cairn: tasks
change: a-move-is-delivered-once
---

# Tasks

- [x] Live test reproducing the double delivery against Stalwart: tests/moves.rs bounces a message between two mailboxes; without the fix it failed on bounce 1 or 2 in each of three runs.
- [x] Find which half should stand down, and where the run learns the other delivered: neither half is wrong alone. Phase 1 scans collections over several connections, and two pushes overlapping both read the create as pending; with one connection the move delivered once every time.
- [x] Fix: one push at a time per source (`pushing` in `phase1_spine`, held across a collection's push passes), scans and fetches still parallel. tests/moves.rs green on five runs, 12 manual bounces once each; CHANGELOG `### Fixed`; requirement in cairn/spec/sync.md; log entry.
