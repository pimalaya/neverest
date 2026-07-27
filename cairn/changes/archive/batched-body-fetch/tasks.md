---
cairn: tasks
change: batched-body-fetch
---

- [x] io-imap: `ImapMessageFetchStreamBatch` coroutine (`UID FETCH <set> (UID
      BODY.PEEK[])`, per-message MessageStart/BodyChunk/WantsStream/MessageEnd,
      UID-from-line routing, UidMissing fallback signal) + unit tests.
- [x] io-imap: `ImapClientStd::fetch_bodies_stream` driver (per-message open/done
      sinks, 128 KB body buffer) + error variant.
- [x] neverest: `Client::fetch_bodies` dispatch + backend `fetch_bodies` (parse
      uids, select-cached, inner batched stream).
- [x] neverest: `hydrate_batch` (per-message HydrateSink, per-message tick,
      per-message fallback on batch error); `fetch_full` batches + work-steals;
      size probe / largest-first removed.
- [x] Build, fmt, clippy clean (io-imap 207 tests; neverest 14 tests).
- [x] Live Stalwart: 200 msgs → 4 batched FETCHes, markers intact & unique, no
      body mixing, idempotent re-sync (routing correct).
- [ ] Fold delta into `cairn/spec/sync.md`; write log entry.
