---
cairn: tasks
change: the-store-has-a-home-on-every-platform
---

# Tasks

- [x] Split the platform base out of `replica_dir` behind one `state_base`, cfg'd per platform, the join written once.
- [x] Test: the default root ends in `neverest/<account>` and is absolute, and `store.root` still overrides it.
- [x] Type-check the macOS and Windows arms, which the Linux build never compiles.
- [x] config.sample.toml, src/config.rs, CHANGELOG.md and MIGRATION.md stop naming XDG as the only answer.
- [x] Fold the delta into cairn/spec/sync.md; log; land.
