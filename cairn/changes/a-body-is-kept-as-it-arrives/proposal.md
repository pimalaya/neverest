---
cairn: change
id: a-body-is-kept-as-it-arrives
status: landed
created: 2026-10-04
---

# A body is kept as it arrives

## Why

The one-source sync (`run_local`) runs three account-wide phases: spine, hydrate, apply. Phase 2 downloads every missing body of the account into an in-memory cache, and only phase 3 raises the items to `Full` in the store. So:

- no body is readable until the whole account has downloaded;
- a run that stops during phase 2 (the user quits, the machine sleeps, the network drops, the caller's time limit) keeps nothing it downloaded, and the next run starts phase 2 from scratch;
- an account whose bodies take longer than the caller's limit never completes its first sync.

Reported by MOA: on a real Gmail box, no mail opens for about an hour after the box is added (MOA `docs/plan/first-sync.md`, step 1).

## What (design)

- **Each hydrate batch is applied as soon as it is fetched.** The worker that fetched a batch raises its items to `Full` over a cache-backed remote holding only that batch, a miss still falling back to a real fetch on the worker's own connection. Per batch rather than per collection: a collection's batches are spread over the whole queue (largest first), so waiting for a collection's last batch would hold most of a large archive in memory and lose it on interruption, which is the problem.
- **Applies are serialised** through one lock on one store handle. Fetches still overlap across the pool; only the index write is serial, as phase 3 was. Two collections may hold the same body (a Gmail label and All Mail), so two concurrent upgrades would write the same object row.
- **Phase 3 goes.** Nothing is left for it: a batch that failed to apply is warned about and stays below `Full`, picked up by the next run's plan, as a failed collection apply was.
- **A fetch error still stops the pool and fails the run**, but every batch applied before it stays in the store. The next run's plan only holds what is still without a body (already the rule: the plan is the placements with no stored object).
- **No cache** beyond the batches in flight.
- **Order unchanged:** largest first across collections. A caller wanting a collection first runs it alone with `--include-collection` first, as MOA's staged first sync will; a `--first` option is left out until a caller needs it.

## Out of scope

- The two-sided sync (`hydrate_full_collection`), which already applies per collection.
- Progress output beyond the existing spinner (a session mode is its own change).
- Fetching bodies on demand.
