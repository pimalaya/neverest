---
cairn: change
id: neverest-follows-io-webdav-0-9-1
status: landed
created: 2026-11-15
---

# neverest follows io-webdav 0.9.1

## Why

Ledger line W2 of the CLI alignment round of 2026-11-15. io-webdav 0.9.1 (released 2026-11-15, lines W1, W3, W4) refuses an id that would address the server root, the home set or a parent of either before sending anything, returns `address-data` and `calendar-data` whole (iCloud and Fastmail escape each CR as `&#13;`, and the trimmed text cut the final CRLF, so neverest stored every card and object a multiget read two bytes short of a GET), and percent-encodes a relative id.

## What

- Cargo.toml requires io-webdav 0.9.1; Cargo.lock moves io-webdav alone.
- No code of neverest builds a DAV path from an id: `propfind_href` (src/dav/client.rs) follows hrefs the server gave, every other request goes through `WebdavClientStd`, whose 0.9.1 methods check each id. neverest takes no DAV id from its command line (`sync -m` and `-x` filter names, sending nothing): the ids it hands io-webdav come from its listings, from the keys another source's collections have in the store, and from `resource_id`, one segment by construction. So no check of neverest's own is added; a test pins the client's.
- The now-exact bodies change nothing for a store already synced: a member listed at the ETag the store holds is named by its binding and never fetched again (src/offline/remote.rs, "DAV collections enumerate by sync token and resolve at Full"), so the first sync after the upgrade sees no DAV item as changed and downloads nothing again. A body stored short stays so until its server copy changes; it parses, and a merge reads cards and objects parsed, never byte for byte.

## Decisions

- **A refused id is not coded.** carillon and cardamum code it as their collection or item missing (`calendar-not-found`, `addressbook-not-found`, `card-not-found`); neverest has no code for a missing collection and takes no DAV id from its command line, so the refusal fails the collection's or item's row like any I/O failure, io-webdav's reason among its sources. A code would be new vocabulary, left to the owner.
