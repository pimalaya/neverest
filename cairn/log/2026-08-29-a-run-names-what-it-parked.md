---
cairn: log
change: a-run-names-what-it-parked
landed: 2026-08-29
---

# A run names what it parked

`phase1_spine` runs a collection per worker and merges their reports at a barrier. It merged one field of each, `item.patch`, and dropped `conflicts`, `refused` and `collisions` on the floor. Everything the collection had just worked out about what it could not do was discarded before the account's report was printed.

Two things followed, both seen against a real account. The warnings block is driven by the length of those vectors, so a run that had just parked a divergence printed only the store-wide count: how many items are waiting, never which, in which collection, on which source. And `announce_conflicts` returns early when the parked list is empty, so the per-item warning was never logged and the account's configured `conflict.notify` desktop notification could not fire at all. The notification shipped two days ago was unreachable through this path from the day it landed.

The merge is now `SyncReport::absorb`, one place that says what an account-wide arm is: the name, the dry-run flag, the retention sweep and the outstanding count stay behind, everything a collection can fill travels.

The test that hid it, `a_same_field_collision_parks_and_is_still_counted`, asserted on the collection's own report and could not notice it being thrown away one frame up. It now folds through `absorb` and asserts at the account level, where the warning is printed from, and a new test drives the engine rather than a synthesized event: a real sync against the fake mutable remote parks a real conflict, whose `Conflicted` event travels through `itemize_pulled` and `absorb` into a report naming the item in its warnings.

The two-endpoint path is a hole of the same shape and is not fixed here: `reconcile_pass` discards the engine's events rather than merging a report, so a source-and-target account reports no conflicts at all. Deduplicating events across the convergence loop is a different fix and is flagged in the proposal.

Capabilities moved: sync, one new requirement on the report merge.
