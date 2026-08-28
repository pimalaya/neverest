---
cairn: change
id: list-instead-of-query
status: landed
created: 2026-08-28
---

# The fallback enumeration is a listing, not a query

## Why

`dav-without-sync-collection` landed the recovery an address book needs on a
server implementing none of RFC 6578: catch the RFC 3253 §3.6
`DAV:supported-report` precondition and enumerate through `addressbook-query`
instead. Two things it assumed turned out to be wrong, both found against the
same live server.

**The query is the wrong alternative.** A query carries a filter, and a server
evaluates a filter by parsing every card it holds. The server that refuses
`sync-collection` answers the query with

```
HTTP 500 Sabre\VObject\ParseException
Invalid VObject, line 1 did not follow the icalendar/vcard format
```

One malformed card takes the whole enumeration down, so the fallback fails
exactly where it was supposed to rescue the account. A `PROPFIND` at Depth 1
requesting `getetag` lists resources and their ETags out of the store without
reading a single body, so it enumerates past that card, and it costs the server
less besides.

**A truncated listing was reported as a complete snapshot.** The query path set
`complete: true` unconditionally and never looked at a 507 row, which
`from_entry` dropped silently as a collection self-entry. The engine reads a
complete snapshot as "absence means removed", so a server truncating the listing
would have deleted every member it left out.

The classification itself has moved upstream into io-webdav, which now names the
refusal (`WebdavSendError::UnsupportedReport`, `WebdavClientStdError::is_unsupported_report`),
implements the `PROPFIND` enumeration behind a caller-fed flag, and reads
`supported-report-set` while listing collections. This crate's copy of the
status-sniffing classifier can go, and the capability read means a sync run
picks the right enumeration before it sends a REPORT rather than after one
fails.

## What

- The fallback runs io-webdav's `PROPFIND` enumeration
  (`WebdavSyncCollectionOptions { fallback: true }`) rather than
  `addressbook-query`.
- A truncated fallback listing is reconciled as a delta, not as a snapshot.
- The local `is_unsupported_report` / `http_status` / `send_status` classifier is
  replaced by `WebdavClientStdError::is_unsupported_report`.
- The enumeration is chosen from the advertised `supported-report-set` when a
  listing filled io-webdav's cache, and from the refusal otherwise. The cache
  carries over a reconnect, like the principal and home-set URLs it sits beside.

## Not in scope

**No repair of the malformed card.** A collection holding a resource its own
server cannot parse is the operator's problem. Enumerating past it is the whole
ask: the card is listed, its body fetch fails on its own, and the rest of the
address book syncs.
