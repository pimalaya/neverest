---
cairn: log
change: a-move-reports-both-halves
date: 2026-11-20
---

# A move reports both halves

A card moved between two address books of one CardDAV source was reported as its delete alone by a real run (`itemPatch` listing `delete … book`), while the dry run listed the delete and the add, and the server took the PUT. Collections push in turn: the address book the card leaves pushed its tombstone first, a relocation (pimdir SYNC §3, §5) that delivered the pending create, so when the address book it enters was itemized the create was no longer `Created` and no row was left for it.

## What landed

- src/sync/driver/itemize.rs: `relocated_creates` reads the collection as its source loads it, whose tombstones carry their destination, and derives the hunks of each destination still pending as a `Created` placement; `itemize_single` and `itemize` list them beside the tombstone's delete. A dry run derives none, each create then reported by its own collection, which nothing delivered. `itemize` takes both sides' stores to read each side's destinations.
- src/sync/driver/pair.rs: passes both stores.
- CHANGELOG under Fixed.

## Spec

- [sync](../spec/sync.md): *A create is named by what delivers it* gains the relocated create and its scenario.

## Tests

src/sync/driver/tests.rs `a_moved_card_is_itemized_as_both_its_halves`: a card moved from `contacts` to `other`; the real run itemizes `delete in contacts` and `add in other` from `contacts`, the dry run the delete alone. With `relocated_creates` deriving nothing it fails (`["delete in contacts"]`). The CardDAV ctx of `a_store_only_flag_on_a_card_is_not_itemized` moved into the shared `carddav_ctx`.

Checked on a loopback CardDAV server, both orders: a move into a later address book and one into an earlier, each reporting the delete and the add once, the next run reporting nothing.
