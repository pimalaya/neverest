---
cairn: tasks
change: a-message-is-keyed-by-the-formats-reading
---

# Tasks

- [x] The ten fixtures under tests/vectors/fixtures and their vector rows (src/kind/mail.rs, `vectors`)
- [x] The old readings seen failing on them: IMAP `ENVELOPE`, Gmail, Graph (see the log)
- [x] IMAP `fetch_summaries` over the listing's header fields; the `ENVELOPE` readers and rfc2047-decoder removed
- [x] Gmail `message_meta` over a header block; the import's `Message-ID` match by `message_id`
- [x] Graph `message_meta`, `internetMessageId` by `message_id`
- [x] `ItemSummary`, `Address`, `parse_summary` and the two parsers removed; `Client::fetch_summaries` answers derivations
- [x] Spec, CHANGELOG.md, log
- [x] `cargo fmt`, clippy on every feature set of the log, `cargo test --all-features --bin neverest`
- [ ] Graph reads `internetMessageHeaders`: waits for io-msgraph to model the property (the owner)
- [ ] A JMAP connector reads `header:…:asRaw:all`, first element: with the connector
