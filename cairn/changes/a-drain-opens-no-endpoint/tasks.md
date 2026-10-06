---
cairn: tasks
change: a-drain-opens-no-endpoint
---

# Tasks

- [x] `offline/driver`: one drain at the start of `run`, before `Account::resolve`, not under `--declare-only`; the drains of `run_local` and `run_pair` removed.
- [x] `cli/drain`: `neverest drain`, `DrainOutput` (`busy`, `applied` with `seq`, `parked`, `waiting`), exit 3 when busy; `cli/sync`: `try_store_lock`; schema `neverest-drain`.
- [x] tests/drain.rs: a drain and a sync apply a queued create with no server; a busy store is left alone; on Stalwart, the drained event keeps its `seq` across the push and the fetch back.
- [x] Spec folded, log written, CHANGELOG.
- [x] Verify: `cargo test`, `cargo clippy`, `cargo fmt`; Stalwart and Radicale suites (create, caldav, carddav, moves).
