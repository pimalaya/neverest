---
cairn: log
change: an-unsent-message-is-not-success
date: 2026-10-30
---

# An unsent message is not success

## What landed

- src/sync/report.rs: `left_waiting` counts a parked send, a parked intent and a parked queue action, so each exits 2; `unsettled` (JSON `unsettled`, omitted when empty, and an "Unsettled" block in the text report) names the collections whose last convergence pass still moved, and `incomplete` counts it (exit 3).
- src/offline/driver.rs: both convergence loops (the one-source sync and the two-endpoint reconcile) record a collection that did not settle within `MAX_EXTRA_PASSES`.
- src/cli/sync.rs, README.md: the exit codes as they now read.
- Tests: `a_parked_send_waits_for_a_person` (replacing `a_parked_send_is_not_incomplete`, which locked in exit 0), `a_parked_intent_or_queue_action_waits_for_a_person`, `an_unsettled_run_is_incomplete`; on the old code they read `Success` where `Conflicted` or `Incomplete` is due.

## Capabilities moved

- sync: a conflicted run succeeds, with its own exit code (modified: parked sends, intents and queue actions); an incomplete run has its own exit code (modified: a collection still moving).
