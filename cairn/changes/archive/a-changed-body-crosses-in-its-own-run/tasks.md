---
cairn: tasks
change: a-changed-body-crosses-in-its-own-run
---

- [x] Reproduce it by hand against two CardDAV principals, both `retain` modes, from a clean store each time, and record what each run reported against what each server held
- [x] Read the propagate ordering to confirm the pass sequence leaves room for the fix: the opening round pulls, `propagate` hydrates, `itemize` reads, then the pushing passes loop
- [x] `hydration_targets` (src/offline/storage.rs) takes the second shape: an item both endpoints hold, no shared body, exactly one base body lost
- [x] Gate the new shape on the far side's `item.update` rather than its `item.create`, which the `HydrationSide` argument now carries alongside the name
- [x] Carry the declared authority in too, so a difference both endpoints made under `one-way` hydrates the deciding side instead of falling through to a conflict path that does not run
- [x] `a_one_way_account_overwrites_the_target_instead_of_parking_the_divergence` in tests/endpoints.rs is the regression witness: it failed before, passes now, and its `#[ignore]` reason is back to naming only the server it needs
- [x] Re-run the live reproduction in both `retain` modes: the update lands in the run that observed it and the run after is quiet
- [x] `cargo test --all-features`, the whole live suite one binary at a time, `cargo clippy --all-features --all-targets`, `cargo fmt`
- [x] Fold the delta into cairn/spec/sync.md and write the cairn/log entry
