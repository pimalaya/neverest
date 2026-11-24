---
cairn: log
change: the-live-night-findings
landed: 2026-11-24
---

# A collection deleted on the server is retired

Task N5 of the-live-night-findings, under the owner's ruling R3 of 2026-11-24: a collection deleted on the server is retired and reported. On the live night of 2026-11-23 (journal 22:33), an IMAP `DELETE` of a folder holding one message left it in the store with its message, `collectionPatch: []`, even with `collection.delete = true`.

`retire_gone` (src/sync/driver/local.rs) runs in a one-source run after the listing is recorded: each collection under the source's namespace the listing no longer names, bound by no other source, is dropped through io-pimdir's `delete_collection` and reported as a new `CollectionHunk::Retire` (JSON `kind: "retire"`, text `retire collection <name>, deleted on <source>`). `delete_collection` refuses a collection a queued or parked row anchors on (`PimdirError::Queued`): it stays, and the hunk is reported as a `collection-patch` error that waits for a person (src/sync/report.rs `waits_for_a_person`, exit 3). A listing naming no collection retires nothing; `--declare-only` retires nothing. No io-pimdir API was missing. Capability moved: sync.md, new "A collection deleted on a lone source's server is retired".

## Checked

- tests/pair.rs `a_book_deleted_on_the_server_is_retired_and_reported` (tests/fakedav gains a spare book and `delete_book`): without the `retire_gone` call `collectionPatch` stayed empty (`left: 0 right: 1`); with it the book is retired once, and a parked `Remove` keeps it, exit 3.
- Live on Stalwart (rc neverestrc-stalwart, lock `neverest-store-stalwart`): `CREATE lane-a2-n5` and one message appended, a sync fetched it; IMAP `DELETE lane-a2-n5`; the next sync printed `retire collection lane-a2-n5, deleted on imap`, with four collections the live night had deleted on the server (`live-2026-11-23-cardamum`, `live-2026-11-23-carillon` on carddav, `live-2026-11-23-himalaya-pd`, `-pd2` on imap), exit 0; the sync after reported `collectionPatch: []`.
