---
cairn: log
change: intents-the-rights-and-the-format-refuse
date: 2026-11-17
---

# Intents the rights and the format refuse

Three parts of finding NEV-19 of the CLI family analysis of 2026-11-15.

## What landed

- src/sync/driver/queue.rs: `perform_collection_intent` refuses (parks) a collection intent whose right the source's configuration now withholds (`collection.create`, `collection.rename`, `collection.delete`). `sync --declare-only` skips the drain that would have refused it, so a `collection-delete` queued while the right was on was performed after it was turned off. Test tests/pair.rs `a_declaration_performs_no_collection_intent_its_rights_now_refuse` over the fake CardDAV server, which now deletes its book: the book was deleted on the old code.
- src/sync/invitation.rs: the private `v: 1` invitation decoder refuses an unknown member (`deny_unknown_fields`), as every io-pimdir reader does: a row with a misspelled `recurence_id` was performed on the whole series. Test `a_misspelled_member_parks_rather_than_acting_on_the_series`.
- src/imap/backend.rs: the dead `copy_messages` deleted (every IMAP copy is an upload; a server-side copy is a later change, not this fix).
- Left of NEV-19: an anchored floating series keeps a floating `RRULE` `UNTIL` (plausible, not reproduced).

## Capabilities moved

- [sync](../spec/sync.md): *A declaration syncs no item* amended.
