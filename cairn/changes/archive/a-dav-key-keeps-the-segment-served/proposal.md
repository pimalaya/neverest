---
cairn: change
id: a-dav-key-keeps-the-segment-served
status: landed
created: 2026-11-15
---

# A DAV key keeps the segment the server served

## Why

Ledger line W10 of the CLI alignment round of 2026-11-15 (.github/cairn/changes/cli-alignment-2026-11-15/tasks.md), neverest's part, with W6 for the rename. io-webdav 0.10 (W7) decodes every id it reads from an href when encoding it gives the segment back (`My%20Cal` read as `My Cal`), and reads a raw `My Cal` a non-conforming server serves as the same id. neverest keys its store on the segment the server served, so taking those ids would move every escaped key, a collection or a member read as gone and another as new. io-webdav 0.11 (W9, released) carries the href as served beside every listed, enumerated, multiget and written id; its last segment, query and fragment dropped, is the 0.9.1 id byte for byte.

**The W9 hold is lifted.** The decision of the-alignment-housekeeping, that neverest stays on io-webdav 0.9.1 until io-webdav exposes the served href of a listed entry, is met by io-webdav 0.11: neverest moves to it, keyed on the href, and no store key changes.

## What

- Cargo.toml requires io-webdav "0.11.0"; Cargo.lock moves io-webdav alone.
- src/dav/client.rs:
  - `list_collections`: a collection's key is io-webdav's id when it starts with a slash (a path outside the home set, kept as served), else its href's last segment (`listed_key`, `served_segment`).
  - `collection_key` (the default calendar a scheduling inbox names): the same, over the URL's path, since `collection_id` now decodes the segment form.
  - `reports`: io-webdav caches the reports under its id, so the key is turned into it (`webdav_id`: a key holding no `/`, `?` or `#` read through `collection_id("", key)`, the way the listing reads the segment; any other as it stands).
  - `fetch_bodies`: each body and each gone row named by `href_id` of its href, the way the enumeration names members.
  - `create` and `move_item`: the written id is the last segment of `created.href` and `moved.href`.
  - `rename_collection`, CalDAV arm: a `CaldavCalendarPatch` setting the display name alone (W6).
  - `propfind_href` runs its coroutine through `WebdavClientStd::run`; neverest's own `run` is gone.
- No other place takes an io-webdav id as a key: `sync-collection` deltas carry hrefs (read by `href_id`, unchanged), reads, updates and deletes return no id neverest keeps, and a create of a collection addresses the key neverest hands it.

## Decisions

- **Key on the href, not on the id encoded again.** An earlier attempt (W8, held) encoded io-webdav 0.10's decoded id again through `collection_path` and `join_path`. That restores an escaped segment, but encodes a raw space or non-ASCII byte a server served, so its key moved and two spellings io-webdav reads as one id collided. The href gives the segment back exactly.
- **A multiget row is matched as the enumeration names members.** 0.9.1 named a multiget row by its raw last segment, the enumeration by `href_id` (A resource has one spelling). Both agree wherever a server spells a segment the one way `href_id` writes it, every escape upper case, an unreserved octet raw and a `:` escaped; where it does not (a raw `:`, a lowercase escape, an escaped unreserved octet), 0.9.1 answered the body under an id no handle had and read the member again alone. Matching through `href_id` answers it under its handle in the one multiget.
- **The reports are found through io-webdav's own reading.** `collection_id("", key)` reads a segment key exactly as io-webdav's listing read the segment it came from, so the lookup needs no state. Two spellings io-webdav reads as one id (`My%20Cal`, `My Cal`) share one cached report set, io-webdav keeping one per id.
- **A rename sends the display name alone**, as before: `CaldavCalendarPatch` leaves the description, colour and time zone untouched (`None`).

## Scope / non-goals

- A moved member keeps the segment io-webdav sends in its `Destination`, encoded: a member a server serves with a raw space comes back encoded after a move, as under 0.9.1, which took the segment of the URL sent.
- No version bump: the release is the owner's.
