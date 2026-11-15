---
cairn: log
change: a-dav-key-keeps-the-segment-served
date: 2026-11-15
---

# A DAV key keeps the segment the server served

Ledger line W10 of the CLI alignment round of 2026-11-15, neverest's part (with W6). The W9 hold is lifted: io-webdav 0.11 carries the served href neverest waited for.

## What landed

- Cargo.toml: io-webdav "0.11.0"; Cargo.lock moved io-webdav alone.
- src/dav/client.rs: every key taken from io-webdav is read from the href it serves beside the id (`listed_key`, `served_segment`), multiget rows through `href_id` as the enumeration reads them, the cached reports found through io-webdav's own reading of the key (`webdav_id`); the CalDAV rename sends a `CaldavCalendarPatch` of the display name alone; `propfind_href` runs through `WebdavClientStd::run`, which resends on a closed connection, and neverest's `run` is gone.
- Six tests, each failing with the keys taken from io-webdav's decoded id, over escaped segments, a raw space and raw non-ASCII.

## Upgrade

No store key moves: a key is the segment io-webdav 0.9.1 gave, so the first sync after the upgrade sees no collection or member as new or gone and downloads nothing again.

## Left

- Two spellings io-webdav reads as one id (`My%20Cal`, a raw `My Cal`) share one cached report set: io-webdav keeps one per id.
- A member moved on a server serving raw names comes back under the encoded segment io-webdav sent, as under 0.9.1.

## Capabilities moved

- [sync](../spec/sync.md): *A DAV key keeps the segment the server served* added; *A collection is keyed by its backend id and named by its display name* amended.
