---
cairn: tasks
change: relay-mode
---

# Tasks

- [x] `config`: `StoreConfig.retention` = `Retain | Relay`.
- [x] `offline/pipe.rs`: bounded cross-thread pipe (`Write` end → `Read` end),
      unit-tested.
- [x] `driver`: retention decision (relay default for IMAP↔IMAP, else retain;
      explicit `retain`/`relay` honoured, non-IMAP relay falls back); `propagate`
      dispatches `hydrate_copies` (retain) vs `relay_copies` (relay).
- [x] `driver`: `relay_targets` (from hub, size from `v:1` meta), `relay_copies`,
      `relay_one` (scoped-thread fetch→append through the pipe, length-prefixed).
- [x] `tests/stalwart2.sh` (two servers) + `tests/relay.rs` (message crosses A→B,
      store keeps zero blobs).
- [x] Verify: relay test green; retain (`stalwart.rs`) unregressed; fmt/clippy.
- [x] Fold `delta.md` into `cairn/spec/sync.md`; log; land.
