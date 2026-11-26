---
cairn: log
change: the-live-night-3-fixes
landed: 2026-11-26
---

# The live night 3 fixes

The live night of 2026-11-25 (.lanes/live/report-3.md §1, §4) fixed four bugs in a lane copy based on c21569a, mixed with a port to the io-pimdir candidate. The port landed another way in ef809a9 (log 2026-11-26-io-pimdir-1-0); the four fixes are carried here by hand onto io-pimdir 1.0.0, each test failing on ef809a9 before its fix.

## What landed

- **A DAV listing names only the collection's members** (the one silent store loss of the night, on the local Stalwart, found by cardamum). Stalwart's `sync-collection` for one address book lists another book's tombstone in `vanished`; neverest keyed every vanished href by its last segment, so a same-named card of the listed book was pulled as deleted and the checkpoint moved past it. src/dav/client.rs `DavClient::keep_members` keeps a changed, removed or unreadable row only where `is_member` finds its href's parent path is the requested collection's (`collection_url`), segment by segment, every escape decoded (`decoded_segment`), and warns of any other, for `sync-collection` and `PROPFIND` listings alike. Capability moved: sync.md, "A DAV listing names only the collection's members" (added).
- **A reset passes over the attachment stand-ins.** `sync --reset` was refused on any mail account holding an attachment: `pending_work` (src/cli/sync.rs) read the stand-ins of the account's attachments collection (pimdir STORAGE §14.3), which no source binds, as unpushed creates. It now passes over the collection of role `attachments`, which the reset drops with the account's other collections and the resync files again. Capability moved: sync.md, "A reset never loses work no server holds" (a sentence and a scenario).
- **A Graph move states the id it gave its message.** A store move to Deleted Items of a second message stating a `Message-ID` another holds (a minted key) dropped the id Graph's `move` answered; the destination's create, which no listing lands under a minted key, was uploaded as MIME by the next push and refused outside Drafts on every run. `GraphClient::move_messages` (src/msgraph/client.rs) answers each moved id with its new one and `Client::move_items` (src/client.rs) passes the pairs on, so io-pimdir lands the create at once (pimdir SYNC §5). Capability moved: sync.md, "Graph mail moves, copies and adds drafts" (modified, a scenario added).
- **A Gmail label move states the id it keeps.** A store move into another label waited for the next listing; a message moved away on the server first stayed a ghost in the label for good (pimdir SYNC §6). `GmailClient::move_items` (src/gmail/client.rs) answers every message it moved under its own id, none for one gone already (`modify` answers the ids it modified, `modify_one` whether the message was there), and `Client::move_items` passes them on. Capability moved: sync.md, the Gmail source requirement (labels and moves).
- CHANGELOG.md: four Fixed lines under 1.0.0.

## Checked

- tests/pair.rs `a_delta_naming_another_book_s_members_leaves_this_book_as_it_is` (fake DAV faults `foreign_removed`, `foreign_changed` in tests/fakedav/mod.rs) failed before (`left: ["b"]`, `right: ["a", "b"]`); src/dav/client.rs `a_member_is_told_by_its_parent_path`.
- src/cli/sync.rs `a_reset_passes_over_the_attachment_stand_ins` failed before (`work/attachments: 1 unpushed change(s)`).
- src/sync/remote.rs `a_graph_move_states_the_handle_it_gave_its_member` failed before (`left: None`, `right: Some(PimdirHandle("m1-moved"))`).
- src/client.rs `a_gmail_label_move_answers_each_moved_message_with_its_own_id` failed before (`left: []`, `right: [("m1", "m1")]`).
- Live on the local throwaway Stalwart (2026-11-26, a scratch store): one card name in two new books, deleted in one. Stalwart's raw `sync-collection` of the other book listed the deleted card's href with a 404; neverest pulled the deletion in the book it was deleted from only, kept the other book's card, warned `CardDAV server listed .../live-2026-11-26-nev-a/y.vcf outside live-2026-11-26-nev-b in its listing, skipped` (and the same for the account's `default` book), and the next run found nothing to do. Both books deleted after.
