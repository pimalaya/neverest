---
cairn: log
change: neverest-follows-io-webdav-0-9-1
date: 2026-11-15
---

# neverest follows io-webdav 0.9.1

Ledger line W2 of the CLI alignment round of 2026-11-15, neverest's part.

## What landed

- Cargo.toml: io-webdav "0.9.1"; Cargo.lock moved io-webdav alone.
- src/dav/client.rs: test `an_id_climbing_out_of_its_collection_sends_nothing`, failing on 0.9.0 (the MKCOL of `..` sent and answered 201), passing on 0.9.1.
- No check of neverest's own: it builds no DAV path from an id and takes none from its command line; io-webdav's client checks every id it is handed.

## The exact bodies

A card or object a multiget reads from iCloud or Fastmail now keeps its final CRLF. A store already synced sees no change: a member listed at the ETag the store holds is named by its binding, its body never fetched again, so no item reads as changed and nothing is downloaded again. The bodies stored two bytes short stay so until their server copy changes; they parse, and merges compare parsed cards and objects.

## Capabilities moved

- [sync](../spec/sync.md): *A DAV id stays inside what it names* added; *DAV collections enumerate by sync token and resolve at Full* amended.
