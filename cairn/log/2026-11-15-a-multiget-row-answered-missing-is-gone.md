---
cairn: log
change: a-multiget-row-answered-missing-is-gone
date: 2026-11-15
---

# A multiget row answered missing is gone

io-webdav 777d34e (pinned in place of 185ab63) makes `multiget_cards` and `multiget_items` return `CarddavCardMultigetOk` and `CaldavItemMultigetOk`, the entries plus `unreadable`, the rows that came without data. neverest no longer compiled against it (src/dav/client.rs, `fetch_bodies`).

## What landed

- `DavClient::fetch_bodies` answers the ids of the `unreadable` rows whose response status is 404 or 410 as gone, as the Graph and IMAP paths already did, where it answered none and left each to a lone `GET`. Any other unreadable row (a 403, a refused or empty data property) stays unanswered, logged at debug, so the lone read of hydrate_batch reports what it is.
- src/offline/remote.rs: `a_member_missing_from_every_read_is_gone` became two tests, a 404 row gone in one request, and a 403 row read alone then gone on its 404.
- tests/relay.rs: the doc comment no longer names the removed `store.retention`.

## Capabilities moved

- [sync](../spec/sync.md): *A body fetch answers for every handle it was asked about* gains the multiget rule.
