---
cairn: tasks
change: a-body-stands-for-its-attachments
---

# Tasks

- [x] io-pimdir `[patch.crates-io]` to `14a8fca`, `Cargo.lock` moved by that line alone.
- [x] Every path landing a body checked to reach `write::apply`.
- [x] `AttachmentMark`, `BodyScan`; the sink and `stored_item` derive the mark; `Kind::parse_body` takes it.
- [x] Unit tests: the mark of a streamed body, cut in 1, 7, 64 octets or whole, and of a stored body, equals the whole message's (two attachments, an inline image, a nested multipart, a top-level attachment, a part no delimiter ends).
- [x] Stalwart (tests/attachments.rs): stand-ins and marks after a primed sync, a paged one over four connections and one after the listing over one; nothing new on a second sync or for a copy; a body left owed records its stand-ins in the run landing it, an arrival under `--bodies listed` first.
- [x] Spec folded, log written, CHANGELOG.
- [x] `cargo test` (default, `--all-features`, MOA's set), clippy `-D warnings` on the same sets, the Stalwart suites (heads, hydration, scope, moves, drain, submit, create, attachments).
