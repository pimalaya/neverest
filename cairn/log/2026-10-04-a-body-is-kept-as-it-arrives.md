---
cairn: log
change: a-body-is-kept-as-it-arrives
date: 2026-10-04
---

# A body is kept as it arrives

The one-source sync downloaded every body of the account into memory, then raised them all to `Full`. No mail was readable before the whole account had downloaded (about an hour on a real Gmail box, reported by MOA), and a run stopped halfway kept nothing, the next one starting over.

## What landed

- src/offline/driver.rs: phase 2 applies each batch as soon as it is fetched (`hydrate_pool`, `apply_batch`), the applies serialised on one store handle; phase 3 removed. A fetch error still fails the run, every batch applied before it kept.
- src/offline/remote.rs: `CachedFetchRemote` serves one batch, generic over its fallback.
- Unit test: a pool whose fetch fails on its third batch keeps the first two, through no wire call.
- tests/hydration.rs (Stalwart, :143): a run killed halfway through its second batch keeps the first; the next run fetches only the rest. It fails on the previous driver (nothing kept).
- Not run: tests/msgraph.rs and tests/google.rs (no credentials where this landed).

## Capabilities moved

- sync: the one-source sync runs as two account-wide phases, each body kept as it arrives.
