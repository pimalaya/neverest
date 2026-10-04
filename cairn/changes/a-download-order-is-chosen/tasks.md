---
cairn: tasks
change: a-download-order-is-chosen
---

# Tasks

- [x] `cli/sync`: `--download-order <largest|newest>`, default `largest`, passed to `driver::run`.
- [x] `offline/driver`: plans carry each body's date; `hydrate_batches` orders by the chosen order.
- [x] Unit tests: largest first unchanged; newest first within and across collections, dateless last.
- [x] Stalwart: a run with `--download-order newest` syncs whole.
- [x] Spec folded, log written, CHANGELOG.
- [x] Verify: `cargo test`, `cargo clippy`, `cargo fmt`.
