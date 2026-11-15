---
cairn: change
id: an-unsent-message-is-not-success
status: landed
created: 2026-10-30
---

# An unsent message is not success

## Why

Pimdir stable-release review (2026-10-30), report honesty. A run exits 0 while it leaves behind work a person must settle:

- a `submit` the server refused definitively (a 550) parks, and the run exits 0 (src/sync/report.rs, `left_waiting`; test `a_parked_send_is_not_incomplete` locks it in): the message the user queued was not sent;
- an intent that parked, and a queue action the drain parked, exit 0 too, though the report lists them;
- a run whose convergence passes end at `MAX_EXTRA_PASSES` still moving (src/offline/driver.rs) returns normally, as if settled.

A report never claims success over a run that did not deliver.

## What

- A parked send, a parked intent and a parked queue action make the run exit 2, the conflict code: each waits for a person, as a conflict does, and a rerun alone changes none of them. A parked queue action is re-reported, and keeps the code, on every run until repaired.
- A run whose last convergence pass still moved something is incomplete (exit 3), named in the report as unsettled.
