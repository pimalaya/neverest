---
cairn: change
id: a-collection-is-named-not-only-addressed
status: landed
created: 2026-09-06
---

# A collection is named, not only addressed

## Why

A hub collection id is `<namespace>/<name>` and the store's `collections.name` has always been a verbatim copy of it, because io-pimdir seeded the column from the id and offered no way to write it. Every store neverest has produced therefore names its collections `imap/Archives`, `carddav/Default`, `caldav/ED99C7C8-2741-11F1-9B88-2C202A48A29D`.

For mail that is merely a prefix a frontend cannot safely strip, the separator being neverest's own convention. For DAV it is worse: a collection is addressed by its path segment, which servers routinely make a UUID, so the last of those three has no readable name anywhere in the store. `src/dav/client.rs` was already fetching what the server calls it, io-webdav parsing `DAV:displayname` on both arms, and dropping the whole struct for its id.

Underneath sits an id/name conflation. `list_collections` collapsed each `Collection` to its `.name` and fed that to `hub_id` and to `wire_name`, so the display name was the addressing key. That works only while the two are equal, which is exactly what has to stop being true.

pimdir's `collection-display-name` closed the format's half: a name is a label, never an address, and `set_collection_name` writes it.

## What

- **Key on the id.** `list_collections` answers the backend id against the display name, and every hub id, every wire call and the account's collection filter are built from the id. On IMAP and on Graph the two are the same string, so nothing moves; on DAV the key stops being the name.
- **Keep the display name.** The DAV listing pairs each path segment with its `DAV:displayname`, blank and absent both falling back to the segment.
- **Write it.** `declare_name` runs beside every `ensure_collection`, on both the solo and the paired path, reading the sources in declared order and taking the first with something to say. Naming a collection is a label: a failure is logged, never raised.

## Not in scope

Cross-side creation still names a new DAV collection after the key rather than carrying the origin's display name over. A collection filter still matches the id, so a DAV collection cannot be filtered by its display name; it never could. `color`, `description` and `sort_order` stay unwritten, io-pimdir having no setter for them either.
