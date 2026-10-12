---
cairn: tasks
change: one-way-overwrites-a-diverged-target
---

# Tasks

- [x] Reproduce with `cargo test --features dav --test endpoints a_one_way -- --ignored` on `./tests/radicale.sh`, with `--log-level trace` on the second sync.
- [x] Find which layer records the conflict, and fix it there.
- [x] The test passes, and the other three in tests/endpoints.rs still do.
- [x] CHANGELOG `Fixed`, log entry, status `landed`.
