---
cairn: change
id: sources-register-through-io-pimdir
status: active
created: 2026-11-26
---

# Sources register through io-pimdir

Carried from follows-storage-draft-17 (archived 2026-11-26), needs-io-pimdir.md 2.

## Why

Before it declares capabilities, a run records its source as syncing the collections it is about to sync ("A calendar its server keeps from tasks is declared so", cairn/spec/sync.md). src/sync/storage.rs `register_syncing` does it by opening its own SQLite connection to pimdir.db and running io-pimdir's `sql::LOAD_CHECKPOINT` and `sql::UPSERT_CHECKPOINT` itself, because io-pimdir had no owner write for it. io-pimdir 1.0.0 has one, `PimdirSourceStore::register`, which adds a `sources` row with no checkpoint only where none is held, under the store's own lock. The raw connection bypasses that lock and ties neverest to io-pimdir's SQL text.

The live night of 2026-11-25 made this move in its lane (.lanes/night3/neverest, src/sync/storage.rs and src/sync/driver/local.rs), not carried with the night's fixes.

## What

- src/sync/driver/local.rs and src/sync/driver/pair.rs: register through the source's own store handle (`PimdirSourceStore::register`); `register_syncing`, its raw connection and the `rusqlite` and `sql` imports go from src/sync/storage.rs.
- Check that `register` (which also runs `ENSURE_COLLECTION`) changes nothing for collections the run did not list.
- Spec: the requirement names `PimdirSourceStore::register` instead of the canonical `load_checkpoint` and `upsert_checkpoint` in neverest's own transaction.
