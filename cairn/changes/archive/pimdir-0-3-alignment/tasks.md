---
cairn: tasks
change: pimdir-0-3-alignment
---

# Tasks

- [x] Cargo: bump the io-* dependencies, add `io-sasl` (with `scram`), enable
      `io-imap/scram` so the SCRAM-SHA-256 the config offers is runnable.
- [x] SASL: build credentials from io-sasl, leaving the SCRAM nonce empty for
      io-imap to draw; the wizard offers only the mechanisms the config spells.
- [x] Clients: IMAP and SMTP through their command traits, the session options
      structs, `Stream` for Graph, io-webdav's renamed types.
- [x] Sort key: `Kind::parse_body` / `parse_summary` derive one per kind and
      the remote seam carries it on every fetched item.
- [x] Hash: `HydrateSink` folds the store's hasher; `offline::hash` deleted.
- [x] Account: every store handle opens `for_account`.
- [x] Probed items upgrade to the tier their kind resolves at (`Full` for DAV).
- [x] Hydration targets key on the missing body, not on the level.
- [x] CardDAV: reopen a connection the server closed and run the exchange again.
- [x] Store refusal names `sync --reset`.
- [x] Tests: the mail sort key is UTC and agrees across tiers; a card's is its
      casefolded name; the CardDAV end-to-end test creates its address book.
- [x] `cargo test --all-features`, `cargo clippy --all-features --all-targets`,
      `cargo fmt`; the CardDAV run against a live Radicale.
- [x] Fold `delta.md` into `cairn/spec/sync.md`; add the `cairn/log` entry;
      mark the change `landed` and archive it.
