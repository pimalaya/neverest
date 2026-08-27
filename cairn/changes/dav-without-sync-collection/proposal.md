---
cairn: change
id: dav-without-sync-collection
status: landed
created: 2026-08-28
---

# A DAV server without `sync-collection` is enumerated by query

## Why

Every address book on a deployment that does not implement RFC 6578 was
unsyncable. The enumeration is `REPORT sync-collection`, the server answers that
it does not support the report, and the client had no second way to ask.

The evidence is unambiguous, and was gathered rather than guessed at. The
collection's `supported-report-set`:

```xml
<d:supported-report><d:report><card:addressbook-multiget/></d:report></d:supported-report>
<d:supported-report><d:report><card:addressbook-query/></d:report></d:supported-report>
```

`expand-property`, `principal-property-search` and `principal-search-property-set`
alongside them, and no `sync-collection` anywhere. The REPORT itself, sent both
with and without a trailing slash on the collection URL, answers:

```xml
<d:error xmlns:d="DAV:"><s:exception>…ReportNotSupported</s:exception><d:supported-report/></d:error>
```

Both spellings failing identically rules out the request being malformed, which
was the competing hypothesis: the URL is right and the server simply has no such
report. `DAV:supported-report` is the RFC 3253 §3.6 precondition that says so by
name.

The client already recovers from a rejected *token* with a fresh full report.
There was no recovery for a server that never had the report at all.

## What

An enumeration whose REPORT comes back with the `DAV:supported-report`
precondition falls back to `addressbook-query` (RFC 6352 §8.6), which the same
`supported-report-set` advertises and which yields the same member ids and
ETags.

The fallback carries no sync token, so its checkpoint is empty and an empty
checkpoint reads as no cursor: such a collection enumerates in full on every
run, which is what a server offering nothing incremental costs.

Detection is on the precondition, not on the status. The status a server wraps it
in is its own choice, so matching one would be guessing; `405` and `501` are
taken on the status alone, both meaning the request was never going to run.

The status is read wherever the failed send nested it. A `sync-collection`
REPORT wraps its send one level deeper than a plain request
(`WebdavSyncCollection(Send(HttpStatus))` against `Send(HttpStatus)`), so a match
written against the outer variants alone compiles, passes a test built the same
way, and silently misses the only path that enumerates. The test carries both
shapes and names which one the REPORT actually produces.

## Not in scope

**No capability probe at connect time.** The refusal could be predicted from
`supported-report-set`, at the price of a round-trip per session for something
the first failed response names for free. One wasted request against one kind of
server is cheaper.
