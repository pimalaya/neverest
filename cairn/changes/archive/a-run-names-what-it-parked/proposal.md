---
cairn: change
id: a-run-names-what-it-parked
status: landed
created: 2026-08-29
---

# A run names what it parked

## Why

`collection_spine` fills a per-collection `SyncReport` and returns it. `phase1_spine`, which runs those collections across a worker pool, merged exactly one field of it back:

    merged_ref.lock().unwrap().item.patch.extend(rep.item.patch);

`conflicts`, `refused` and `collisions` were dropped on the floor, so everything `itemize_pulled` and `itemize_refused` had just worked out inside the collection was discarded before the account report was printed.

Two consequences, both observed against a real account. The `Warnings` block is driven by the length of those vectors, so a run that had just parked a divergence printed only the store-wide count, `Conflicts: 1 item(s) waiting for a decision`, which says how many and never which, in which collection, on which source. And `announce_conflicts` opens with `if report.conflicts.is_empty() { return; }`, so the per-item warning was never logged and the account's configured `conflict.notify` desktop notification could never fire at all. The feature documented in config.sample.toml was unreachable through this path. A create a source refused because it already held the identity was lost the same way.

The test that should have caught it, `a_same_field_collision_parks_and_is_still_counted`, asserted on the collection's own report and could not see it thrown away one frame up.

## What

- `SyncReport::absorb` folds a collection-scoped report into the account's, every arm of it, in one place. What does not travel is what a collection never had an opinion about: the account name, the dry-run flag, the retention sweep and the outstanding count, which is read once from the store rather than summed.
- `phase1_spine` absorbs both at the worker barrier and at the account fold.
- The two conflict tests now assert at the account level, and a new one drives the engine itself: a real `sync` against the fake mutable remote produces a real `Conflicted` event, which travels through `itemize_pulled` and `absorb` into a report that names the item in its warnings.

## Not in scope

**The two-endpoint path.** `reconcile_pass` discards the engine's events rather than merging a report, so a source-and-target account reports no conflicts at all and cannot notify either. That is a hole of the same shape and a different fix: the events would have to be itemized per pass and deduplicated across the convergence loop, which is not a report-merge bug and is not surgical. Flagged rather than folded in.
