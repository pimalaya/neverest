---
cairn: change
id: pimdir-0-3-alignment
status: landed
created: 2026-08-24
---

# Realign on io-replica 0.4, io-pimdir 0.3 and the io-sasl split

## Why

The Pimalaya libraries moved under neverest: io-replica 0.4 gave a placement a
sort key and made a flag set tell *unknown* from *known-empty*, io-pimdir 0.3
made the store own the hash its objects are named by (and refuses a store an
earlier draft wrote), pimalaya-stream 0.3 dropped its SASL module into the new
io-sasl crate, and io-imap 0.6 / io-smtp 0.3 / io-webdav 0.2 / io-http 0.5
moved their command surfaces onto traits. None of it compiles against the code
as it stands, and two of the changes are not mechanical: the store now expects
the sync to carry a sort key it never derived, and it names bodies by an
algorithm neverest was not using.

The point of the store is that another process reads what this one wrote. A
consumer hashing bodies its own way names them where no other reader looks, and
a consumer writing no sort key leaves every reader scanning a whole collection
into memory to render fifty rows. Both are silent: nothing errors, the dedup
simply never dedups and the listing simply has no order.

## What

- Follow the API moves: SASL credentials from io-sasl, the IMAP/SMTP command
  surfaces through their client traits, the session options structs, io-webdav's
  renamed types, and pimalaya-stream's `Stream`.
- Derive a **sort key** per kind at the same seam that derives the link id and
  the summary: the `Date:` header in RFC 3339 UTC for mail (both tiers agreeing,
  as they must for the link id), the casefolded `FN` for a card.
- Take the content hash from the **store** (`PimdirStore::blobs`), deleting
  neverest's own FNV digest, so a body is named the way the store records it.
- Group every collection under the syncing **account**, so a store two
  hand-written accounts share says whose collection is whose.
- Answer a store the format outgrew with the command that fixes it, rather than
  with the raw refusal.

## Scope / non-goals

- The CardDAV connection repair and the probe-tier fix below are bugs the live
  run surfaced while verifying the bump; they are in scope because the backend
  is unusable without them.
- A remote content change on a mutable-content backend still leaves the item
  bodiless: the hub keeps the level a source reached, so no upgrade refetches
  it. That is an io-replica fix, not a neverest one, and it is written down in
  the log rather than worked around here.
