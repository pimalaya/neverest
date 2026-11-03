---
cairn: log
change: neverest-follows-the-freeze-gaps
date: 2026-11-03
---

# neverest follows the freeze gaps

io-pimdir 4e64216 (pimdir 6687cf4) holds back the `Update` of a local body over a base naming a revision and no body, until an upgrade at `Full` reads that base. neverest's hydration picked only bodiless placements, so such an edit would have waited for good: `bodiless` (src/offline/driver.rs) now also owes a placement whose `awaits_base`, and so does the full hydration. Guarded by `an_edit_over_an_unread_base_is_owed_its_base_body`, which fails without the change (`left: []`).

`PimdirRemoteSnapshot` gains `capped`: neverest's connectors know of no listing cap, so every page states `false`. A bodiless copy or move is refused unless its source syncs the target, which `PimdirSourceStore::mutate` reads from `collection_sources`: nothing to do here.

Pinned at io-pimdir 0c70144, whose drain states `syncs_target` for a queued `move` or `copy` as well. `cargo test --all-features` 499 passed.
