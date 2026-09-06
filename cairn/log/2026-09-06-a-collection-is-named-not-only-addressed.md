---
cairn: log
change: a-collection-is-named-not-only-addressed
landed: 2026-09-06
---

# A collection is named, not only addressed

Every store neverest has written names its collections after their hub ids: `imap/Archives`, `carddav/Default`, `caldav/ED99C7C8-2741-11F1-9B88-2C202A48A29D`. io-pimdir seeded `collections.name` from the id and had no setter, so the namespace landed in the label too, and on DAV, where a collection is addressed by a path segment servers routinely make a UUID, the label said nothing at all.

The display name was there the whole time. io-webdav parses `DAV:displayname` on both arms and `src/dav/client.rs` dropped the whole `CarddavAddressbook` / `CaldavCalendar` for its id, then set the name to that id. Behind it sat the reason it had to: `list_collections` collapsed each `Collection` to its `.name`, and that string became the hub id and the wire name, so the display name *was* the key. Putting a real name in it would have addressed DAV collections by their labels.

So the key moved first. `list_collections` now answers the backend id against the display name; `filter_collections`, `hub_id` and every wire call read the key. On IMAP and on Graph the two strings are equal and nothing moved; on DAV the key is the path segment it always should have been, which is also what a configuration's collection filter names.

Then the name follows. The DAV listing pairs each segment with its `DAV:displayname`, blank and absent alike falling back to the segment, and `declare_name` writes it beside every `ensure_collection` on both the solo and the paired path, reading the sources in declared order and taking the first with something to say. Nothing keys on the column, so a refused write is logged and the run carries on: naming a collection cannot be worth failing a sync over.

A store now reads `imap/Archives` named `Archives`, `carddav/Default` named `Contacts`, `caldav/ED99C7C8-…` named `Work`.

Left alone deliberately: cross-side creation still names a new DAV collection after the key rather than carrying the origin's label over, and a collection filter still matches the id, so a DAV collection cannot be filtered by its display name. It never could.

Verified with `cargo test --bins` (151 green, including a CalDAV collection keeping its `caldav/` id while its name reads `Work`, and a silent source falling back to the bare id), `cargo clippy --all-targets`, `cargo fmt`, and a read-back through `PimdirReader` showing the three shapes side by side.
