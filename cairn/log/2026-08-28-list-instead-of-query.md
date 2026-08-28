---
cairn: log
change: list-instead-of-query
landed: 2026-08-28
---

# The fallback enumeration is a listing, not a query

`dav-without-sync-collection` landed this morning and was wrong twice, both found against the same live server it was written for.

**The query it fell back to fails on the very collection it was meant to rescue.** `addressbook-query` carries a filter, and a server evaluates a filter by parsing every card it holds. The server that refuses `sync-collection` answers the query with an HTTP 500 `Sabre\VObject\ParseException`, one malformed card taking the whole enumeration down. The fallback now runs io-webdav's `PROPFIND` enumeration at Depth 1 (`WebdavSyncCollectionOptions { fallback: true }`), which reads names and ETags out of the store and parses nothing, so the collection lists past that card and only its own body fetch fails. It is cheaper on the server besides, ETags being the whole point of an enumeration.

**A truncated listing was reported as a complete snapshot.** `query` set `complete: true` unconditionally, and the 507 row that says otherwise (RFC 6578 §3.6) was dropped silently by the self-entry filter. The engine reads a complete snapshot as "absence means removed", so a server truncating the listing would have deleted every member it left out. io-webdav now carries the flag on both enumeration paths, and a truncated listing is reconciled as a delta: nothing is deleted, and removal detection waits for a round the server answers whole.

**The classifier moved upstream.** io-webdav names the refusal itself now (`WebdavSendError::UnsupportedReport` on the RFC 3253 §3.6 precondition, plus `405` and `501`), so the local `is_unsupported_report`, `http_status` and `send_status` are gone for `WebdavClientStdError::is_unsupported_report`, and the test that pinned their behaviour went with them: which reports a server advertises is the library's reading, tested there against the same real body.

**The choice is now made before a REPORT is sent.** io-webdav reads `supported-report-set` while listing collections and caches it per collection, and a sync run lists both sides before it enumerates either, so the capability is free by the time `enumerate` runs. An address book advertising no `sync-collection` is listed straight away; one nobody listed still pays the one failed REPORT, whose refusal names itself. The cache carries over a reconnect, beside the principal and home-set URLs it sits with.

Capabilities moved: sync, where the query requirement was replaced by the listing one.
