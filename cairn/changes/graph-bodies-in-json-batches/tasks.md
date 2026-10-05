---
cairn: tasks
change: graph-bodies-in-json-batches
---

# Tasks

- [x] `msgraph/client`: `fetch_messages` over `$batch`, bodies decoded and committed in request order, throttled requests retried.
- [x] Unit tests: request ids and URLs; out-of-order bodies (both base64 alphabets), throttled ones retried in order with the longest `Retry-After`, refused and undecodable ones left out; a batch throttled as a whole.
- [x] Live: tests/msgraph.rs (11) green on the test tenant; a whole-mailbox sync stores the same 51 objects as the previous build, through 4 batches and no single get.
- [x] Spec folded, log written, CHANGELOG.
- [x] Verify: `cargo test`, `cargo clippy`, `cargo fmt`.
