---
cairn: tasks
change: an-open-imap-round-lists-its-head
---

# Tasks

- [x] `client`: `heads()` answers `BOTH` with QRESYNC, `BAND` without, for IMAP.
- [x] `offline/driver`: the sync coroutine is told the heads the source lists.
- [x] `imap/backend`: delta head through the QRESYNC delta (`qresync_delta`, shared with `delta_page`), `NoHead` and `HeadRejected` as designed.
- [x] `imap/backend`: band head by `UID SEARCH SINCE`, read as a page reads (`list_uids`).
- [x] `imap/backend`: the round cursor carries the highest UID found; the last page of a resumed round lists again what arrived above it; an 8-byte cursor reads it as 0.
- [x] `imap/client`: `NEVEREST_IMAP_NO_QRESYNC`, debug builds only.
- [x] Unit tests: cursor, QRESYNC position, trust by `UIDVALIDITY`, the band head's day.
- [x] tests/heads.rs (Stalwart): a delta head lands an arrival and an expunge while the round is open, the round closes keeping it, the next delta lists nothing; a band head lands the arrival, the round's close finds nothing absent (a dry run names no removal, which fails without the last page's re-listing), the next round drops the expunged message.
