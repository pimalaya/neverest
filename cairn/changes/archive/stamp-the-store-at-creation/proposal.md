---
cairn: change
id: stamp-the-store-at-creation
status: landed
created: 2026-08-26
---

# A store this version had just created was refused as the ancestor

## Why

`StoreState::load` refuses a store directory holding a `pimdir.db` with no `neverest.json` beside it, reading it as the unnamespaced ancestor whose collections would all be looked up under keys nothing was ever written to. The premise is that only an older neverest leaves that pair, and it was never true: nothing wrote the sidecar at creation.

`neverest init` opens the store, which materializes `pimdir.db`, and stops there. The next run, dry or not, loads the sidecar before anything else and finds none, so a fresh account was refused on its very first sync:

    Error: The store at `…` was written before collection ids carried their namespace, and is not read. Drop it with `neverest sync --reset` and let it resync…

The remedy the refusal names does not work either. `reset_replica` recreates the empty store the same way, sidecar and all still missing, so `sync --reset` succeeds and the next run raises the same refusal, pointing at the reset that just ran. A new account had no path out of it short of reading the source.

`--dry-run` is where it surfaces first, since a dry run copies the store into a temporary directory and the copy is what the refusal names, but the run it copies from is equally refused.

## What

- `StoreState::stamp` writes a default sidecar, stamping the current layout and clearing whatever a previous store in the same directory derived. Creating `pimdir.db` and stamping the layout it was written with are one act; the two call sites that materialize a store now do both.
- `neverest init` stamps the store it creates.
- `sync --reset` stamps the store it recreates, so the remedy the refusal names actually clears it. Clearing the recorded derivations is right here too: a reset store has no previous run to have derived anything.
