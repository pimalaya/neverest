---
cairn: log
change: gtd-hardening
date: 2026-10-29
---

# A filed task moves on its server, a calendar is its path

neverest's part of unfog's cairn/changes/gtd-hardening. An `Add` carrying `supersedes` (io-pimdir 8907303, pimdir SYNC §3 and §4) is delivered on CalDAV by a WebDAV `MOVE` of the superseded member, then a write of the stored body over it, gated on the ETag the move answered; a `405`, `501` or `404` falls back to the copy or the upload, a duplicate `UID` refuses the create. A DAV delete answered `404` or `410` is accepted, so the held delete of the moved source passes. `move_items` moves with `MOVE` where the server can. A calendar outside the home is keyed by its path, as io-webdav b2b9a00 lists it, the default calendar too.

Capabilities moved: sync (added A create superseding a member is delivered by moving it, A delete of a member already gone is accepted, A DAV relocation moves the member; modified A collection is keyed by its backend id and named by its display name, A CalDAV source syncs calendars).

Verified: `cargo test` 424 green, fmt and clippy clean, 15 of the 17 new tests failing on the earlier code; live through unfog's tests/pimdir-path.sh on 2026-10-29, on Radicale and on Fastmail behind the guard: a task filed into another calendar is moved there on the next sync, its source gone, nothing refused, where the upload Fastmail refused for its one `UID` per account before.
