---
cairn: log
change: an-incomplete-run-says-so
date: 2026-11-17
---

# An incomplete run says so

Finding NEV-6 of the CLI family analysis of 2026-11-15: a run a signal stopped (exit 4) printed "Account dav is already in sync" under its "Interrupted:" block, the verdict chosen from hunks, errors and warnings alone, and `unreached` and `throttled` had no text block.

## What landed

- src/sync/report.rs: an incomplete run with no error reads "is incomplete, rerun it to go on" or "partially synchronized: N hunks, rerun it to go on"; `Unreached (n):` and `Throttled (n):` blocks.
- Test `an_incomplete_run_says_so_in_the_text`.
- Trivial fix: no proposal.

## Capabilities moved

- [sync](../spec/sync.md): *An incomplete run has its own exit code* amended.
