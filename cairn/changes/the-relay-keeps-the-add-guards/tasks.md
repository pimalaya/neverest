---
cairn: tasks
change: the-relay-keeps-the-add-guards
---

# Tasks

- [x] `offline/driver`: `relay_targets` skips a target with an open round, or that never closed one, on the collection, and a message dated outside the target's covered scope.
- [x] Tests, failing first: a target whose round is open gets no relay; a target with a closed round covering the message gets it; a message outside the target's scope is held back.
- [x] Spec folded, log written, CHANGELOG.
- [x] Verify: `cargo fmt`, `cargo clippy --all-features --all-targets -- -D warnings`, `cargo test --all-features`.
