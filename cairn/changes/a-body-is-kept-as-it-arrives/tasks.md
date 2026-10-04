---
cairn: tasks
change: a-body-is-kept-as-it-arrives
---

# Tasks

- [x] `offline/driver`: phase 2 applies each batch as it is fetched, under one store lock; phase 3 removed.
- [x] Unit test: a pool whose fetch fails after N batches leaves the first N batches' bodies in the store, and nothing after.
- [x] Stalwart: a box with several folders syncs whole, every item `Full`; an interrupted run resumes without downloading again what it kept.
- [ ] `tests/msgraph.rs`, `tests/google.rs` live.
- [x] Spec folded, log written, CHANGELOG.
- [x] Verify: `cargo test`, `cargo clippy`, `cargo fmt`.
