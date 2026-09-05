---
cairn: tasks
change: the-engine-lives-in-the-store
---

- [x] Read io-pimdir's README, lib.rs header, changelog, cairn/spec and the pimdir standard (OVERVIEW, STORAGE, SYNC, GUIDE) before touching code
- [x] Cargo.toml: drop io-replica, keep io-pimdir 0.4 with `client` alone, add the `[patch.crates-io]` path entry
- [x] src/offline/mod.rs: the driver services storage yields through `PimdirSourceStore::service`, timing every yield kind
- [x] src/offline/storage.rs: delete `HeldStore`, keep the multi-source reads (`load_side`, `projection_view`, `hydration_targets`)
- [x] src/offline/remote.rs: `PimdirRemote` over the client pool, `PimdirFetchedItem.summary` from the kind's derivation
- [x] src/offline/driver.rs: `Pimdir*` names, `Stale` in the reset remedy, the rekey through the plain driver with the generation read back, the relay size off the mail summary, the merge staging `Update { seq, object }`
- [x] src/kind/: delete vcard.rs and ical.rs, delegate to `io_pimdir::summary`, build `PimdirMailSummary` from the envelope, keep `split_link_id` as the read side of a minted key
- [x] src/item/summary.rs and the IMAP backend: carry `Cc` and `Bcc` so the envelope tier writes the same address rows as the body tier
- [x] src/conflict.rs, src/cli/conflict.rs, src/offline/submit.rs, src/cli/{sync,init}.rs: the `client::` paths, the summaryless `Update`, the timestampless enqueue
- [x] tests/: the `client::` paths, the summaryless `Update`, the timestampless enqueue, the checkpoint drop through the store's own `write`
- [x] io-pimdir: `load` under `Links` answers with the rows the source binds, regression test in tests/engine/hub.rs, requirement in cairn/spec/store.md, changelog line; `cargo test --all-features` and clippy green there
- [x] `cargo build`, `cargo test`, `cargo clippy --all-targets`, `cargo fmt`
- [x] Every live suite against the local Radicale and the two Stalwarts, one binary at a time
- [x] Pull-only live check against the Fastmail account from a fresh store: init, sync, sync again, read back with `pimdir` and `conflict list`
- [x] Fold the delta into cairn/spec/sync.md, write the log entry, CHANGELOG, MIGRATION.md, CONTRIBUTING.md
