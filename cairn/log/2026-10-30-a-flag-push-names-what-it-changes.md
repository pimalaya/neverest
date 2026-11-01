---
cairn: log
change: a-flag-push-names-what-it-changes
date: 2026-10-30
---

# A flag push names what it changes

io-pimdir's `PimdirChangeKind::SetFlags` became an add and a remove set (pimdir SYNC §4); neverest pushed a whole set with a replace, overwriting a flag another client changed after the listing.

## What landed

- src/client.rs: `Client::update_flags(collection, ids, add, remove)` in place of `store_flags`; src/offline/remote.rs pushes `SetFlags` through it.
- src/imap/backend.rs: `update_flags`, a `+FLAGS` then a `-FLAGS` store, each skipped when empty. src/imap/fake.rs applies `UID STORE` and exposes the flags it holds.
- src/msgraph/client.rs: `update_flags` patches only `isRead` and `flag` as the two sets touch them (`flags_delta`), sending nothing for a change touching neither; the full-set `store_flags` is gone.
- src/gmail/client.rs: `update_flags`, one modify naming only the labels the two sets map to; the test session records request bodies.
- src/dav/client.rs, src/gcal/client.rs, src/gpeople/client.rs: `update_flags` refuses, as `store_flags` did.
- src/item/flag.rs: `FlagOp::Set` is left for a create.
- Tests: `a_flag_the_server_added_after_the_listing_survives_a_push` (IMAP), `a_flag_push_patches_only_what_it_changes` and `a_flag_push_naming_nothing_graph_maps_sends_nothing` (Graph), `a_flag_push_names_only_the_labels_it_changes` (Gmail).

## Evidence

Each connector test ran first through the old push, a replace with the set the old engine pushed for "listed seen and flagged, `\Flagged` removed": IMAP held `["\\Seen"]` where `["\\Seen", "\\Answered"]` was due; Graph patched `{"flag": {"flagStatus": "notFlagged"}, "isRead": true}`, overwriting an unread mark; Gmail sent `{"addLabelIds": [], "removeLabelIds": ["UNREAD", "STARRED", "IMPORTANT"]}`.

## Capabilities moved

- sync: a flag push names what it adds and removes (added).
