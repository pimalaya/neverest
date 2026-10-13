---
cairn: log
change: an-open-imap-round-lists-its-head
date: 2026-10-13
---

# An open IMAP round lists its head

A run resuming an open IMAP round lists the round's head first (pimdir `round-head`, io-pimdir `07bd316`), so what arrives or leaves during a long first sync shows at once rather than at the first delta after the round closes.

## What landed

- src/client.rs: `heads()` answers `BOTH` for IMAP with QRESYNC, `BAND` without.
- src/offline/driver.rs: `sync_side` tells the sync coroutine the heads the source lists (the base never did, so no head was asked).
- src/imap/backend.rs: `delta_head` and `delta_page` share `qresync_delta` (trusting the store's handles only under its checkpoint's `UIDVALIDITY`); `band_head` by `UID SEARCH SINCE`; `list_uids` reads a UID set as every page does; `RoundCursor` carries the highest UID found when the round opened, and the last page of a resumed round lists again what arrived above it; `search_round` takes a UID floor and ceiling.
- src/imap/client.rs: `NEVEREST_IMAP_NO_QRESYNC`, read by debug builds only.
- tests/heads.rs: the delta and band heads against Stalwart.

## Capabilities moved

- sync: an open IMAP round lists its head; the IMAP round cursor carries the highest UID found when the round opened.
