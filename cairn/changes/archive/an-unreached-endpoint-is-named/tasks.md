---
cairn: tasks
change: an-unreached-endpoint-is-named
---

# Tasks

- [x] `offline/driver`: `Unreached` marker on `Pool::open` failures (one-source contexts, paired source and target); `run` records `unreached` when the failure carries it.
- [x] `sync/report`: `unreached: Vec<UnreachedEndpoint>`, carried by `absorb`.
- [x] Tests: unit, the marker read through later context and absent from another failure; tests/drain.rs, a closed port named in `unreached`.
- [x] Spec folded, log written, CHANGELOG.
- [x] Verify: `cargo test`, `cargo clippy`, `cargo fmt`.
