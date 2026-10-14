---
cairn: tasks
change: a-body-is-walked-once
---

# Tasks

- [x] Cases checked: a blob missing at write time, a write not through `apply`, a read of the mark neverest computed, a body held by rows from before the rule.
- [x] `AttachmentMark` removed; `parse_body` takes no mark; `BodyScan` reduced to `HeaderScan`; `HydrateSink` takes no kind; `stored_item` reads the header block alone.
- [x] Unit tests: a streamed body, cut in 1, 7, 64 octets or whole, and a stored body are named, sized and sorted as the whole message, the mark left to the store.
- [x] Stalwart: attachments (marks and stand-ins, primed, paged, after the listing, a copy, a body landing later), hydration, moves, heads, relay, duplicates, deletes, scope, submit.
- [x] Spec folded, log written, CHANGELOG.
- [x] clippy `-D warnings` (default, `--all-features`), `cargo test` (default, `--all-features`).
