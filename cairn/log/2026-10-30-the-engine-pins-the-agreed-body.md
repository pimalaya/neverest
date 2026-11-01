---
cairn: log
change: the-engine-pins-the-agreed-body
date: 2026-10-30
---

# The engine pins the agreed body

io-pimdir's the-agreed-body-stays-pinned keeps the body two endpoints agreed on until every binding reconciled and records their divergence itself (pimdir SYNC §9), so neverest's own detection went.

## What landed

- src/offline/driver.rs: `reconcile_pass` runs the two sides' reconciles and nothing else. Gone: `shared_bodies` and `hub_divergences` (the in-memory snapshots), `diverged_items` and `hub_conflicts`, `park_divergences` and `park_hub_conflicts` with their two raw `UpsertPlacement` writes, `parks_divergences`. `bind_relayed` is the one raw placement write left; the hub reads left are those io-pimdir has no verb for (relay targets, projection, hydration targets, the reset's pending work).
- Tests, over the engine alone with a fake CardDAV server per endpoint: `a_card_both_endpoints_rewrote_diverges_against_their_agreed_body` (both endpoints pull a rewrite of a card they agreed on: the right one parks with the agreed body as its base, and the merge settles the disjoint edits) and `a_card_two_endpoints_never_agreed_on_parks_with_no_ancestor` (two bodies before any agreement: the right one parks with no base, nothing merged). Both are the shapes the workaround handled (logs 2026-08-30 endpoints-reconcile-with-each-other and a-divergence-with-no-ancestor-parks).
- The whole suite passes with the workaround removed (465 passed, 67 ignored). tests/endpoints.rs, which drives both cases against two Radicale principals, is ignored by default and was not run.

## Capabilities moved

- sync: two endpoints are reconciled with each other, a divergence with no common ancestor parks rather than resolving itself (modified: the store records the divergence; neverest keeps no snapshot).
