---
cairn: log
change: neverest-follows-sync-draft-07
date: 2026-11-01
---

# A resolution names its conflict

io-pimdir fd0c060 (pimdir 3d6a04e) refuses an edit over a conflict unless it states `resolves`, the diverging bodies it was decided over, and parks a queued `update` that does not. `conflict resolve` (src/conflict.rs) states the remote side the person was shown; the merger (src/offline/driver.rs, the merged body staged over a conflict) states the remote side it merged; the one-way overwrite stays an overrule. Every other update neverest's tests queue is an ordinary edit, `resolves` absent.

Left: `conflict list` reads the bindings' conflicts alone (`list_conflicts`). An item two sources diverged on (`list_item_conflicts`) is neither listed nor resolvable here, and a resolution of its binding parks while the item's own conflict stands, rather than settling a body nobody saw. Listing item conflicts and naming them in the resolution is a change of its own.

`cargo test --all-features` 489 passed on io-pimdir fd0c060.
