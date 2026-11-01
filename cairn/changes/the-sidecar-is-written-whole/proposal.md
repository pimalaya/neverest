---
cairn: change
id: the-sidecar-is-written-whole
status: landed
created: 2026-10-30
---

# The sidecar is written whole

## Why

Pimdir stable-release review (2026-10-30), robustness. neverest.json, the sidecar beside pimdir.db holding the layout, the accepted mode and the looked-up roles, is written with a plain `fs::write` (src/offline/state.rs, `StoreState::save`). A crash or a full disk mid-write leaves it torn, and a torn sidecar beside a pimdir.db is refused with `--reset` as the remedy (`StoreState::load`): one crash leads the user to drop the store.

## What

- The sidecar is written to a temporary file in the store directory, synced, then renamed over the old one, as the in-flight send record already is (src/offline/submit.rs): a reader sees the old sidecar or the new one, never a torn one.

## Not done

- Moving the mode guard and `taskless` into the store, where pimdir wants every guard (guards must persist in the store): needs a store-side place for owner state, an io-pimdir question.
