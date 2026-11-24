---
cairn: log
change: the-live-night-findings
landed: 2026-11-24
---

# A sent copy follows a rename performed in the same run

Task N4 of the-live-night-findings. On the live night of 2026-11-23 (fastmail-sep), a mailbox create, two adds, `message compose --send --save sep` and a rename of `sep` to `sep2` were queued in one batch. The first drain renamed the folder and delivered the message but reported no submission. The second parked it unconfirmed ("a run stopped while handing it over"), and the copy was never filed.

Cause: a run drains the collection intents before the sends. The rename gave the store collection its new id (`follow_rename`), and after the send the intent was replaced by the copy's `add` on the payload's old id, which the store refused (`FOREIGN KEY constraint failed`). The send was left in flight, so the next run parked it unconfirmed.

`drain_collection_intents` now answers the ids its renames moved (`Renames`), and `drain_submits` files the copy under the new one (src/sync/driver/send.rs `acknowledge_sent`). Capability moved: sync.md, "A sent copy is filed once sent".

Left open: a send still pending after the run that renamed its copy folder (an unreached SMTP server, say) names an id gone from the store. io-pimdir's `rename_collection` carries a queued move's target and should carry a `submit`'s `copy` the same way: reported to the io-pimdir lane.

## Checked

- src/sync/driver/collection_intent_tests.rs `a_send_files_its_copy_in_the_collection_renamed_beside_it`: fails without the rename map (`FOREIGN KEY constraint failed`), passes with it.
- Not re-run live: no himalaya on this machine both stages pimdir writes and queues a mailbox rename.
