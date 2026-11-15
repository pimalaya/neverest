---
cairn: tasks
change: a-truncated-dav-listing-is-incomplete
---

# Tasks

- [x] `dav`: `sync` returns incomplete when its last answer was truncated, stops on a truncated answer without a token, keeps the starting checkpoint then.
- [x] Tests, failing first, over a scripted io-webdav stream: a first listing truncated past `MAX_SYNC_ROUNDS` is incomplete; a truncated answer without a token is incomplete and not listed again; a truncation drained to its end is complete.
- [x] Spec folded, log written, CHANGELOG.
- [x] Verify: `cargo fmt`, `cargo clippy --all-features --all-targets -- -D warnings`, `cargo test --all-features`.
