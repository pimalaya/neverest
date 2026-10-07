---
cairn: tasks
change: scoped-mail-sync
---

# Tasks

- [x] Bump io-pimdir to `35a1c3f` and io-msgraph to `00ef069`; `PimRemote` and `CachedFetchRemote` on the new seam; probes and `upgrade_probed` removed.
- [x] `client`: `Enumeration` (pages, cursor, checkpoint, bytes), `EnumEntry::meta`, `Listed`, `Held`, `Client::scope_bound`.
- [x] `imap`: rounds of 500 UIDs newest first, `SENTSINCE`/`SENTBEFORE` with a day of margin, undated messages searched apart, header fields with `Content-Type`, no `BODYSTRUCTURE`, cursor and checkpoint, deltas naming the unbound.
- [x] `msgraph`: summary `$select`, `$filter` two days below the floor, `maxpagesize` 1,000 on every link, page per response, `CursorRejected` on 410, `hasAttachments`; contacts delta at 1,000.
- [x] `gmail`: rounds of 100 ids under `after:`/`before:`, `historyId` first, page token cursor, metadata for the unbound, flag sets for the bound, history deltas naming the unbound, `Content-Type`.
- [x] DAV, Google, Graph contacts and events: bodies fetched per page by `PimRemote`, the bound named by their binding, a dry run fetching none.
- [x] `item.filter.since`, `sync --since`, refused beside contacts and calendars by key.
- [x] Report: `coverage`, `downloaded`; an open round is incomplete.
- [x] Throttle: `Request::{Idempotent, Create}`, creates retried on 429 only; Graph 5xx batch bodies not throttles.
- [x] Tests: unit (scope filter, IMAP meta and cursor, Graph page and filter, Gmail meta and query, config, report, throttle, interrupted and refused rounds); tests/scope.rs on Stalwart; existing suites.
- [x] Spec folded, log written, CHANGELOG, sample configuration.
