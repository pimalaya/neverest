---
cairn: delta
change: sources-register-through-io-pimdir
---

# Delta

## ADDED Requirements

None.

## MODIFIED Requirements

### Requirement: A calendar its server keeps from tasks is declared so

Its third paragraph: the `sources` row with no checkpoint is written through io-pimdir's `PimdirSourceStore::register` on the source's own store handle, one owner write adding a row only where none is held, so a checkpoint already there is never overwritten; no longer through the canonical `upsert_checkpoint` after `load_checkpoint` in neverest's own transaction.

## REMOVED Requirements

None.
