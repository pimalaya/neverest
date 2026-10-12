---
cairn: log
change: one-way-overwrites-a-diverged-target
date: 2026-10-11
---

# A one-way account overwrites a diverged target

Neither lead held. The target's divergence is a binding conflict its own pull records: the pulling pass runs with `push = false`, and io-pimdir's `PreferLocal` falls back to `Manual` when the source may not push (`resolve_conflict`), so the pass parks it. The pushing pass then finds a `Conflict` at the revision it already recorded and derives nothing, so the target kept its edit and the run exited 2.

## What landed

- src/offline/driver.rs: `resolve_conflicts` hands a side whose `Authority` is `Store` to `keep_store_bodies` instead of the merge, which restates each conflicted placement's own body through the engine's `Edit` (`PimdirSourceStore::mutate`). The edit clears the conflict against the revision the endpoint holds, as `conflict resolve --prefer-local` does, and the next pushing pass overwrites the target, gated on that revision. Not through the queue, which refuses an owner edit on a one-way account.
- Tests: `a_side_the_store_decides_keeps_the_store_body_against_the_endpoint` (unit, no server); tests/endpoints.rs `a_one_way_account_overwrites_the_target_instead_of_parking_the_divergence` passes on Radicale, the other four too.

## Capabilities moved

None: sync's "`one-way` declares authority, not silence" already required it.
