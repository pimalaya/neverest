---
cairn: tasks
change: the-releases-of-2026-11-19
---

# Tasks

- [x] io-imap 0.12.0 (`io-imap/client` to `io-imap/std`), io-smtp 0.10.0 (`io-smtp/client` to `io-smtp/std`), io-sasl 0.3.0, io-pimdir 0.18.0, pimalaya-cli 0.7.0, ical-rs 0.7.2; Cargo.lock moves those six alone; `cargo tree` holds io-sasl 0.3.0 alone
- [x] `ImapClient::raw` (now `Vec<Vec<u8>>`): neverest calls it nowhere
- [x] pimalaya-cli 0.7 `request-origin`: neverest has no `request` command, nothing to rename
- [x] C5 helpers of io-pimdir 0.18 (`accounts_of`, `scoped`, `media_type`, `unread_counted`, `overlay_quiet`): the deletions are himalaya's, cardamum's and calendula's; neverest restates none
- [x] Store format: io-pimdir 0.17 and 0.18 both write storage-draft-14 (`FORMAT_VERSION`), their schema and statements identical, so a 0.17 store opens as is; neverest names no sync vector format 7 fixture (tests/vectors holds summary fixtures)
- [x] P5: `purge_account` over `purge_items`, one transaction a sweep
- [x] Test: src/sync/driver/tests.rs `the_retention_sweep_purges_in_one_transaction`, counting the commit frames the store's WAL gains (1), failing at 3 with the old per-item purge
- [ ] N2 (public in-flight guard on `PimdirSourceStore`): not released in io-pimdir 0.18, left
- [x] cairn/spec/sync.md folded, log, CHANGELOG
