---
cairn: log
change: dav-without-sync-collection
date: 2026-08-28
---

# A DAV server without `sync-collection` is enumerated by query

Every address book on a deployment that does not implement RFC 6578 was
unsyncable: the enumeration is `REPORT sync-collection`, the server answers that
it does not support the report, and the client had no second way to ask. The
symptom reaching the user was a run reporting `already in sync` over an address
book holding cards, which the scan-failure change fixed separately; this is what
was failing underneath it.

The cause was established rather than guessed, after two wrong guesses that cost
a round each. The collection's `supported-report-set` holds `expand-property`,
`principal-property-search`, `principal-search-property-set`,
`addressbook-multiget` and `addressbook-query`, and no `sync-collection`. The
REPORT itself, sent both with and without a trailing slash on the collection
URL, comes back with `Sabre_DAV_Exception_ReportNotSupported` and the
`DAV:supported-report` precondition. Both spellings failing identically ruled
out the competing hypothesis, that the URL this crate builds for a collection is
malformed for want of a trailing slash: the URL is right and the server has no
such report.

The client already recovered from a rejected *token* with a fresh full report,
and had nothing for a server that never had the report. It now falls back to
`addressbook-query`, which the same `supported-report-set` advertises and which
yields the same ids and ETags. The fallback keeps no token, so its checkpoint is
empty and an empty checkpoint reads as no cursor: such a collection enumerates
in full every run, which is the price of a server with nothing incremental to
offer.

Detection is on the precondition and not on the status. The status a server
wraps it in is its own choice, so matching one would be the same kind of guess
that wasted the two earlier rounds; `405` and `501` are taken on the status
alone, both meaning the request was never going to run. A permission refusal, a
credential failure and a server fault all still surface. The classifier is
tested against the body a real server answers with.

Capabilities moved: **sync** (DAV enumeration).

A postscript, because it cost another round. The first classifier matched
`Send(HttpStatus)` and the redirect variant and nothing else, while a
`sync-collection` REPORT fails as `WebdavSyncCollection(Send(HttpStatus))`, one
level deeper. It compiled, and its test passed, because the test built the error
the same way the classifier read it. The status is now extracted wherever the
send nested it, and the test carries both shapes with the REPORT's own named as
such.
