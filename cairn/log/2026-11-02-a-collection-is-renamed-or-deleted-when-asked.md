---
cairn: log
change: a-collection-is-renamed-or-deleted-when-asked
date: 2026-11-02
---

# A collection is renamed or deleted when asked

Follows pimdir b63b39a through io-pimdir 908db2b: the `collection-rename` and `collection-delete` intents, and the `mail.flags.important` and `mail.flags.junk` rows.

## What landed

- Cargo.toml, Cargo.lock: io-pimdir by git rev 908db2b (mime-rs in place of mail-parser comes with it); the windows-sys 0.61.2 to 0.52.0 flip `cargo update` made reverted by hand.
- src/offline/collection.rs (was src/offline/create.rs): `CollectionIntent` and `Request` read the three kinds, a malformed payload parking.
- src/offline/driver.rs: `drain_collection_intents` performs them in queue order before the listing, in a sync, a declaration and an intents-only run. On an account without targets, `follow_rename` moves the renamed collection and every one under it to their new ids (`rename_collection`) and names them, and a performed delete drops the collection once its row is acknowledged. On a paired account a rename parks and the store follows nothing.
- src/client.rs: `Renamed`, `rename_named_collection`, `delete_named_collection`. src/imap/backend.rs `rename_named_mailbox`, `delete_named_mailbox` over `mailbox_tree` (the `LIST` with `\Noselect` rows), `renamed_mailbox`, `children_of`. src/gmail/client.rs `rename_label` (nested labels the deepest first, a case-only rename allowed), `delete_label`, `units::LABEL_PATCH`. src/dav/client.rs `rename_collection` (`PROPPATCH` of the display name).
- src/offline/capability.rs: `declaration` takes `paired` and the collections stating a role (`fixed`); rename and delete per collection, `none` on a fixed one, `collection.delete` gated by its right, `collection.rename` `none` when paired; Gmail's `mail.flags.important` `full` and `mail.flags.junk` `none`, `mail.flags.keywords` kept `partial` for older readers. src/cli/check.rs passes `paired`.
- config.sample.toml, README.md: `collection.delete` also lets a frontend ask for a delete.

## Checks

`cargo test --all-features`: 498 passed, 68 ignored; clippy clean. New: `a_rename_and_a_delete_decode_their_performer`, `a_malformed_intent_parks` (rename and delete cases), `only_the_collection_kinds_are_read`, `a_rename_and_a_delete_are_declared_on_each_collection_but_a_fixed_one`, `a_delete_is_declared_only_where_collection_delete_is_granted`, `a_paired_account_renames_no_collection`, `a_gmail_source_states_its_important_and_junk_rows`, `a_renamed_mailbox_keeps_its_parent`, `the_mailboxes_under_one_are_found_at_any_depth`, `a_performed_rename_carries_the_collections_under_it`. `a_caldav_source_points_its_intents_at_scheduling` asserted that only `collection.create` is declared per collection; it now admits the three collection intents.

Not run: tests/create.rs `a_queued_folder_is_renamed_then_deleted_on_the_imap_server`, ignored, needs Stalwart on 143 and 8080, which rootless Docker here cannot bind. The Gmail and DAV verbs are covered by no live test.

## Left

- Rename on every endpoint of a pair, so a paired account declares it.
- Graph mail, contact and calendar folders and Google calendars: rename and delete.
- An IMAP `NO` to `RENAME` or `DELETE` carries no status, so it leaves the row pending rather than parking it, as a refused `CREATE` does.

## Capabilities moved

- sync: a collection is renamed or deleted through the queue (added).
- sync: a performed intent leaves a receipt (modified: the two new intents).
- sync: a check reports what each source declares it can do (modified).
- sync: a Gmail source declares `$Important` and `$Junk` under their own rows (added).
