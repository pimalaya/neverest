---
cairn: change
id: a-one-way-overwrite-keeps-what-it-discards
status: landed
created: 2026-10-30
---

# A one-way overwrite keeps what it discards

## Why

The pimdir stable-release plan (2026-10-30) keeps `prefer-*` for the one-way modes "with the losing body pinned and reported", and the CHANGELOG of the window claimed a one-way account keeps the body it overwrites as a retained item. It does not. The target's own edit is a divergence the target's pull records on its binding (`PreferLocal` on a pass that may not push falls back to `Manual`), and `keep_store_bodies` settles it with an `Edit` restating the store's body. io-pimdir's `Edit` settling a divergence makes the diverging body the base and keeps nothing (pimdir SYNC §7): once the overwrite is pushed, nothing pins the target's body and the collector takes it. Only the hub's policies keep a loser (SYNC §9), and the conflict write already moved the target's agreed body to the store's, so no policy meets a divergence there either.

## What

- A test, `a_one_way_overwrite_keeps_the_target_body_as_a_retained_item` (src/offline/driver.rs, over two fake CardDAV endpoints), stating the behaviour: ignored until io-pimdir keeps it. On the io-pimdir of 2026-10-30 morning it finds no retained item (`left: 0, right: 1`).
- neverest already reads the mutation's `Overruled` events (every-store-outcome-is-reported): once io-pimdir keeps the body, it is reported under `overruled` with no other change here.
- The CHANGELOG withdrew the claim until it held, and restores it once it does.

## Unblocked

Blocked until io-pimdir (frozen as of 2026-10-30): an `Edit` settling a divergence whose diverging body it discards keeps that body as a retained item and reports it `Overruled`, as a `PreferRemote` pull keeps the local edit it drops (pimdir SYNC §7, §9), or a mutation states the policy it settles by. Reported to pimdir and io-pimdir. Settled the same day: pimdir SYNC §7, §9 and io-pimdir state it as the mutation's `overrule` flag (their log an-automatic-settle-keeps-what-it-discards), which `keep_store_bodies` sets, fetching an unread diverging body first since an overruling edit over one is refused (`conflict-unread`).
