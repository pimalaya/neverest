---
cairn: log
change: a-graph-add-is-declared-by-folder
date: 2026-11-15
---

# A Graph add is declared by folder

Ledger line N15 of the CLI alignment round of 2026-11-15, from live testing on Microsoft Graph.

## What landed

- src/offline/capability.rs: `drafts_only`; src/offline/driver.rs `declare` passes it the collections stating the drafts role.
- src/msgraph/client.rs: an add outside Drafts fails coded `unsupported`.
- Tests: `a_graph_source_takes_an_add_in_its_drafts_alone`, `an_add_outside_drafts_is_unsupported`, each failing without the change.

## Left

- (c) an item no remote ever held cannot be removed while the source has `item.delete` off: io-pimdir's gate counts its provisional binding as a holder (load_item_capabilities.sql). A library change.

## Capabilities moved

- [sync](../spec/sync.md): *Graph mail moves, copies and adds drafts* amended.
