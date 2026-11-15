---
cairn: change
id: a-collection-is-renamed-or-deleted-when-asked
status: landed
created: 2026-11-02
---

# A collection is renamed or deleted when asked

## Why

pimdir b63b39a (storage-draft-08, untagged) adds the `collection.rename` and `collection.delete` capabilities and their `collection-rename` and `collection-delete` intents, and the `mail.flags.important` and `mail.flags.junk` rows (pimdir cairn/changes/mail-write-names-from-himalaya). io-pimdir 908db2b follows it. A frontend (himalaya `pimdir mailbox rename` and `delete`) queues the intents; neverest, the store's owner, performs them.

## What

- Repin io-pimdir to 908db2b.
- Perform both intents with the creations, before the listing, through IMAP `RENAME` and `DELETE`, Gmail `labels.patch` and `labels.delete`, and a DAV `PROPPATCH` of the display name and `DELETE`.
- A rename keeps the parent. IMAP moves the mailboxes under it itself; Gmail nests labels by name alone, so each nested label is renamed too, the deepest first.
- A delete never takes another collection: a child on the server parks the row.
- On an account syncing its source alone, the store follows: `rename_collection` for the collection and those under it when the id changes (IMAP, Gmail), a drop of the deleted collection once acknowledged.
- Declare both per collection, `none` on a collection stating a role, `collection.delete` gated by the existing `collection.delete` right, `collection.rename` `none` on a paired account.
- Gmail states `mail.flags.important` `full` and `mail.flags.junk` `none`.

## Decisions

- **The delete is gated by `collection.delete`.** It defaults to false, and so does the intent. On an account syncing its source alone the setting has no other effect, so enabling it costs nothing; on a pair it also lets inferred deletes cross. A destructive request stays opt-in, as `collection.create` gates the creation.
- **No rename on a paired account.** The pair matches collections by name: a rename on the source reads there as a delete and a create, which copies every message again and, with `collection.delete` on the target, deletes the old one there. Renaming on every endpoint is left for later.
- **The store drops a deleted collection on an account syncing its source alone.** Without it the collection stays in the store forever: neverest drops nothing a lone source stops listing, since a listing may leave out a collection nobody deleted. An explicit delete is the one case where the absence is known. The row's receipt cascades with the collection, so a producer reads the row as unknown once the store follows; the collection being gone is what confirms it.
- **Gmail keeps `mail.flags.keywords` `partial`, `$Important` only,** so a reader on an older io-pimdir, which reads `$Important` under keywords, is not refused.
