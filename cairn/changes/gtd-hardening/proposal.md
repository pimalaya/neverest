---
cairn: change
id: gtd-hardening
status: landed
created: 2026-10-29
---

# GTD hardening: a filed task moves, a calendar is its path

## Why

Track E of unfog cairn/changes/gtd-hardening (live provider runs), following io-pimdir cairn/changes/gtd-hardening and io-webdav cairn/log/2026-10-29-gtd-hardening.md.

A task filed into another calendar keeps its `UID`: io-pimdir now stages it as an `add` superseding the source item (pimdir STORAGE §15.3), and its sync names the member the create may be moved from (`PimdirChangeKind::Add::supersedes`, pimdir SYNC §3 "Superseded", §4). neverest uploads the body into the target while the source still holds the `UID`, which Fastmail, keeping one `UID` per account, refuses; the same refusal hits `DavClient::move_items`, a create then a delete. io-webdav now moves an item with WebDAV `MOVE` (`WebdavClientStd::move_item`), which keeps the `UID` in one place.

io-webdav also changed the id of a calendar or address book outside the home set to its href's path, starting with a slash: Google lists every calendar as `/caldav/v2/<calendar>/events/`, so every calendar was named `events`, one of them kept and every verb reaching the primary calendar. neverest keys hub collections, wire calls, the filter and the default calendar on that id.

## What

- Pin io-pimdir 8907303902a8d159ad40d27934de802c3afa7d24 and io-webdav b2b9a00462f7d371de3fb419e53288f0a13cee39 in `[patch.crates-io]`.
- A CalDAV push delivers an `Add` carrying `supersedes` by moving the member into the target (`MOVE`, `Overwrite: F`, gated on the member's last-synced ETag), then writing the stored body over it (If-Match on the ETag the move answered, else unconditional), accepted under the moved member's handle with the revision the write left. A `MOVE` answered 405, 501 or 404 (Google) falls back to the plain upload; one refused for the identity (`DuplicateUid`, 412) rejects the add as a refused upload does. CardDAV, which io-webdav moves nothing of, uploads as before; every other backend ignores `supersedes`.
- A DAV delete answered 404 or 410 is accepted: the superseded tombstone's delete comes after the move took the member along.
- `DavClient::move_items` (a `Remove` relocating into `to`) moves with `MOVE`, falling back to a create then a delete where the server cannot.
- A calendar outside the home set is keyed, addressed, filtered and stored by its path (hub id `caldav//caldav/v2/x%40g/events`); the default calendar the scheduling inbox names is keyed as the listing keys it.

## Out

- A live run against Fastmail, Google or Radicale: the user runs it.
- A Google calendar's new id in an existing store: the calendar is listed under its path and syncs as a new collection; the `caldav/events` it was folded into stays as the listing leaves it.
- A paired create of a calendar keyed by a path on the other endpoint (see tasks.md).
