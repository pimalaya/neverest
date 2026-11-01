---
cairn: tasks
change: an-unsent-message-is-not-success
---

# Tasks

- [x] `sync/report`: `left_waiting` counts a parked send, a parked intent and a parked queue action.
- [x] `offline/driver`: a run ending its convergence passes still moving is reported unsettled; `incomplete` counts it.
- [x] Tests, failing first: a parked send, a parked intent and a parked queue action each exit 2; an unsettled run exits 3.
- [x] Spec folded, log written, CHANGELOG.
- [x] Verify: `cargo fmt`, `cargo clippy --all-features --all-targets -- -D warnings`, `cargo test --all-features`.
