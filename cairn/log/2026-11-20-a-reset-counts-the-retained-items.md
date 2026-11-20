---
cairn: log
change: a-reset-counts-the-retained-items
date: 2026-11-20
---

# A reset counts the retained items

## What landed

- src/cli/sync.rs: `pending_work` counted retained items from `load_hub`, whose query (io-pimdir spec/queries/storage/owner/load_items.sql) excludes every row with a `retained_at`, so a plain `sync --reset` dropped them unrefused. It now pages the collection's trash with `PimdirReader::list_retained` and counts the rows whose retention names an instant (bound to no source); the rows a source still binds stay counted as pending deletes from the hub. The unreachable `item.sources.is_empty()` branch is gone, a live item with no binding counting as unpushed.
- CHANGELOG under Fixed.

## Spec

- [sync](../spec/sync.md): unchanged; *A reset never loses work no server holds* already named retained items and its scenario *A forced reset drops staged work* is now met.

## Tests

src/cli/sync.rs `a_reset_refuses_while_an_item_is_retained`: an item whose only binding is dropped as deleted; a plain reset refuses naming `1 retained item(s)` and `--force`, a forced one drops the collection. Against the old `pending_work` it fails, the plain reset succeeding.
