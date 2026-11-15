---
cairn: change
id: google-sources
status: landed
created: 2026-10-01
---

# Google contacts and calendars are native sources

## Why

MOA syncs a Gmail account's contacts and calendars into pimdir, and neverest reached Google only over IMAP. CardDAV and CalDAV against Google work with a bearer token but are Google's bridge over its own APIs, lossier than People and Calendar; io-gpeople and io-gcal cover both APIs, and their `vcard` and `ical` features now carry the projections Cardamum and Calendula use, so neverest can speak them natively.

## What

- `gpeople` and `gcal` backends, each a cargo feature in the default set, sharing a `GoogleConfig` (`tls`, `alpn`, `auth.token`).
- People: one `contacts` collection, sync-token enumeration, etag revisions, per-person reads, writes mirroring Cardamum's (etag-guarded update, `clientData` merge), a create refused when the UID stash was lost.
- Calendar: calendars as collections, a series with its modified instances as one item under a joined revision, sync-token enumeration, reads by `iCalUID`, imports keeping the UID, writes guarded by the joined revision then the series' etag.

## Not in scope

Pushing a locally modified instance of a series; the wizard offering either backend; an end-to-end run against a real Google account.
