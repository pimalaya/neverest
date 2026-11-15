---
cairn: tasks
change: a-collection-is-renamed-or-deleted-when-asked
---

# Tasks

- [x] Repin io-pimdir to 908db2b (Cargo.toml, Cargo.lock, the windows-sys flip reverted)
- [x] src/offline/collection.rs replaces src/offline/create.rs: the three collection intents
- [x] src/offline/driver.rs: `drain_collection_intents`, `follow_rename`, the store drop of a deleted collection, a parked rename on a pair
- [x] src/client.rs `rename_named_collection`, `delete_named_collection`; IMAP, Gmail and DAV verbs
- [x] src/offline/capability.rs: the rows, `none` on a fixed collection, the delete right, the paired rename, Gmail's flag rows
- [x] Unit tests; tests/create.rs `a_queued_folder_is_renamed_then_deleted_on_the_imap_server` (ignored, Stalwart)
- [ ] Run the ignored Stalwart test (needs ./tests/stalwart2.sh, ports 143 and 8080)
- [ ] Rename on every endpoint of a pair
- [ ] Graph mail, contact and calendar folders, Google calendars: rename and delete
