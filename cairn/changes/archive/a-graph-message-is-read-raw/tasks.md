---
cairn: tasks
change: a-graph-message-is-read-raw
---

# Tasks

- [x] io-msgraph pinned at 92c28d5 in `[patch.crates-io]`, Cargo.lock moved by `cargo update -p io-msgraph` alone
- [x] What 7480799 and 92c28d5 moved, read against each use neverest makes of it (see the log)
- [x] `internetMessageHeaders` in `SUMMARY_SELECT` (src/msgraph/client/mail.rs), which every read naming a member passes
- [x] `fetch_metas` over `MsgraphClientStd::message_get`, the request line unchanged
- [x] `message_meta` names a row listing header fields through `derive_meta`, one listing none as before
- [x] A delta keeps a `Named` of a row it names, not the row
- [x] `a_row_listing_its_header_fields_is_named_as_the_vectors_state`, `a_row_listing_its_header_fields_keeps_the_size_and_the_mark_graph_states`, `every_read_naming_a_member_selects_its_header_fields`, each seen failing on the old code
- [x] Spec, CHANGELOG.md, log
- [x] `cargo fmt --check`, clippy on every feature set of the log, `cargo test --all-features --bin neverest`, the five suites that reach no server
- [ ] What a draft answers for `internetMessageHeaders`, and what io-msgraph makes of a `null` there: the owner's, with a live probe
- [ ] The page sizes against Graph with the larger rows: the owner's, with a live run
- [ ] A delta link stored before this change lists no header field: the owner's to refuse, or to leave to the next round
