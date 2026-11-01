---
cairn: change
id: the-engine-pins-the-agreed-body
status: landed
created: 2026-10-30
---

# The engine pins the agreed body

## Why

Pimdir stable-release review (2026-10-30), engine boundary. neverest detects cross-endpoint divergences itself (src/offline/driver.rs:3478-3866): `shared_bodies` snapshots every item's shared body in memory before each pass, `diverged_items` finds items whose shared body and both bases were dropped, and `park_divergences` hand-writes a `Conflict` placement with its base. It exists because a body-less pull drops the shared body (SYNC §5, §9), so §9 cannot see the divergence. The ancestor is never persisted, breaking "guards persist in the store"; the code admits a collector race (:3720-3724); and three raw `PimdirWriteOp::UpsertPlacement` writes (:3435, :3735, :3901) plus direct `load_hub` reads make engine internals a de facto API. `items.conflicted` also outlives the decision (:3557-3570, :3815-3818).

**Later work**: depends on pimdir spec edit 4 (the agreed body stays pinned until every binding reconciled) landing in io-pimdir.

## What

- Once io-pimdir pins the shared body: `shared_bodies`, `diverged_items` and `park_divergences` go, §9 recording the divergence in the engine.
- The raw placement writes shrink to the relay's `bind_relayed`, or go if io-pimdir offers a verb for it.
- `items.conflicted` read as the engine and spec settle when it clears.
- The tests of a-divergence-with-no-ancestor-parks and an-ancestor-less-divergence-parks-again pass unchanged against the engine's own detection.

## Outcome

io-pimdir's the-agreed-body-stays-pinned (2026-10-30) keeps the agreed body pinned until every binding reconciled and records a divergence between the endpoints itself, with or without an ancestor. neverest's snapshot (`shared_bodies`, `hub_divergences`), its detection (`diverged_items`, `hub_conflicts`) and its hand-written `Conflict` placements (`park_divergences`, `park_hub_conflicts`) went, with two of the three raw `UpsertPlacement` writes; `reconcile_pass` runs the two sides' own reconciles alone. The relay's `bind_relayed` stays, io-pimdir offering no verb to record a copy made around the engine, and so do the hub reads io-pimdir has no verb for (the relay targets, the projection the report reads, the hydration targets, the reset's pending work). `items.conflicted` is no longer read by a sync: the engine settles an item conflict by an edit or a remove (SYNC §3, §7) and decides what parks; the reset still reads it as pending work.
