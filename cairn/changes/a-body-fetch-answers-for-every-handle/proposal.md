---
cairn: change
id: a-body-fetch-answers-for-every-handle
status: landed
created: 2026-08-28
---

# A body fetch answers for every handle, or says so

## Why

A CardDAV account was found returning card bodies as zero bytes. The cause was
server-side and is now understood (a Posteo app password authenticates but
cannot unwrap the stored cards), but the way neverest met it was its own defect,
and the same shape would recur against any server that answers for fewer members
than it was asked about.

**A short batch counted as a success.** `hydrate_batch` sent 64 handles to
`addressbook-multiget`, io-webdav dropped every entry whose `address-data` was
empty, and the 2 cards that came back were returned as the batch's result. The
engine recorded those 2, learned nothing about the other 62, and asked for them
again on the next run. Every run enumerated 153 members, fetched them, stored
nothing, and reported "already in sync". The per-item fallback that would have
caught it only ran when the batch *errored*, which this never did.

**An empty body was stored as an item.** A zero-byte body hashes to the digest
of nothing, so every empty card resolved to one link id, `hash:cbf29ce484222325`.
The second one to arrive collided with the first, the duplicate-link-id floor
froze the identity, and the collection was poisoned for every later run. A run
that dropped 152 items on the floor still printed "already in sync".

Neither is about one provider. A backend that answers for a subset is a thing
that happens, and the engine has no way to tell "these 62 have no bodies" from
"these 62 were not asked about" unless the fetch says so.

## What

- A batched fetch that answers for fewer handles than it was asked about falls
  back to a per-item fetch for the remainder, as a batch error already does.
- A body of zero bytes is refused with the item named, rather than stored. No
  kind neverest syncs has an empty body: a message carries headers and a card
  carries at least its `BEGIN` and `END` lines.

## Not in scope

**No per-item tolerance.** An empty body fails the collection rather than being
skipped, so a run cannot report success over an item it could not read. The
alternative, dropping it silently, is the behaviour this change exists to
remove.
