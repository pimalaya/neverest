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
Before phase 1 of a one-source run, a collection the store calls settled under the run's scope, whose source states no change since the checkpoint the store holds, SHALL NOT be listed (pimdir SYNC §5): a Gmail label the mailbox history, read once from the earliest such checkpoint, names no change for, its checkpoint moved in one write to the history id the mailbox stands at; a DAV collection whose `{DAV:}sync-token` in the home-set listing equals its checkpoint. It still reports its coverage and has its owed bodies downloaded. Every other source, IMAP among them, and every pair lists the collection as before.

## MODIFIED Requirements

### Header (sync.md:11)
The store and the engine follow pimdir storage-draft-17, sync-draft-15 and search-draft-17 (io-pimdir cc3021b, then its release): a store of storage-draft-16 or earlier is refused, never migrated, and an index of search-draft-16 or earlier rebuilt.

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

### Requirement: A store error carries its code
`property-invalid` joins the codes neverest cannot meet: it writes no property itself, and the drain parks a `set-property` row it cannot apply. The same in "The rules the code's comments carried".

## REMOVED Requirements

None.
