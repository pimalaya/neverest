---
cairn: delta
change: follows-storage-draft-17
---

# Delta to cairn/spec/sync.md

## ADDED Requirements

### Requirement: A settled collection is listed once a run
After a collection's pull pass, an endpoint SHALL push only where io-pimdir's `PimdirSourceStore::settled_collections` does not name the collection (pimdir SYNC §5): a settled one owes no push, holds no refusal, lacks no item and has no round open, so a push pass would list it again for nothing. A server change made after the pull waits for the next run. In a pair, an endpoint's settledness is read after the other endpoint's pull.

#### Scenario: A quiet mailbox
- GIVEN a writable IMAP source whose INBOX was synced and has not changed
- WHEN the account syncs
- THEN INBOX is selected once (src/sync/driver/tests.rs `a_settled_collection_is_listed_once_a_run`)

### Requirement: A source that states no change leaves a settled collection unsynced
A collection the store calls settled, whose source states no change since the checkpoint the store holds, in the coverage's scope and with no full listing asked, SHALL NOT be listed nor loaded by the run (pimdir SYNC §5): Gmail when the run's account history names nothing of the label, a DAV collection whose `{DAV:}sync-token` in the home-set listing equals its checkpoint's. Every other source lists the collection as before.

## MODIFIED Requirements

### Header (sync.md:11)
The store and the engine follow pimdir storage-draft-17, sync-draft-15 and search-draft-17 (io-pimdir 7f16b27, then its release): a store of storage-draft-16 or earlier is refused, never migrated, and an index of search-draft-16 or earlier rebuilt.

### Requirement: The summary is the format's typed row
The row is the store's one `summary` row of the item, its `component` naming the kind, with its addresses, `In-Reply-To` and categories as `summary_value` rows (pimdir STORAGE Annex A, storage-draft-17), in place of the per-kind tables and `item_address`.

### Requirement: A reset never loses work no server holds
The work a plain reset refuses over SHALL include an item holding a local property (pimdir STORAGE §4.3: a local label among them), which no source carries and which the store drops with the last row of its public id; `--force` drops it with the collections.

#### Scenario: A labelled message
- GIVEN an account whose message carries the label `keep`
- WHEN it is synced with `--reset`
- THEN the run refuses naming `--force` and the label stays; with `--reset --force` it goes (src/cli/sync.rs `a_reset_refuses_while_an_item_holds_a_property`)

### Requirement: A capped source says so, and keeps what it leaves unlisted
Its last paragraph becomes: a rekey from a capped listing SHALL NOT be written while any based binding resolves to no member, whatever their number (pimdir SYNC §8); `--accept-absences` drops each such binding (src/sync/driver/tests.rs `a_capped_renumbered_mailbox_parks_on_one_absence`).

### Requirement: A relocation names the member it made
Its last sentence becomes: neverest states a server object id on Gmail alone, its message id (pimdir SYNC §4, §6), so a Gmail body is never linked from another message stating its `Message-ID`; on IMAP io-imap 0.12.0 can neither request nor parse `EMAILID`, so the listed variant of tests/moves.rs `a_move_into_a_message_stating_its_id_keeps_both` still documents the gap.

### Requirement: A store error carries its code
`property-invalid` joins the codes neverest cannot meet: it writes no property itself, and the drain parks a `set-property` row it cannot apply. The same in "The rules the code's comments carried".

## REMOVED Requirements

None.
