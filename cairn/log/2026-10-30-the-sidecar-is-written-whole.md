---
cairn: log
change: the-sidecar-is-written-whole
date: 2026-10-30
---

# The sidecar is written whole

## What landed

- src/offline/state.rs: `StoreState::save` writes neverest.json.new, syncs it and renames it over neverest.json, as the in-flight send mark is written, where a plain `fs::write` rewrote it in place.
- Test: `a_save_replaces_the_sidecar_whole` (Unix), which on the old code kept the same inode (`left: 48905091, right: 48905091`).

## Not done

- The mode guard and `taskless` stay in the sidecar; moving them into the store waits for a store-side place for owner state.

## Capabilities moved

- sync: the sidecar is replaced whole (added).
