---
cairn: change
id: a-moved-uidvalidity-rekeys-first
status: landed
created: 2026-10-30
---

# A moved UIDVALIDITY rekeys first

## Why

Release blocker B1 of the pimdir stable-release review (2026-10-30). When an IMAP mailbox's `UIDVALIDITY` moves, the connector answers a round of the new UIDs (src/imap/backend.rs:162-170), restarts an open round on a moved cursor, and lists a band head without checking it at all (:251-280). The driver only rekeys after the whole sync, comparing the stored checkpoint's epoch before and after (src/offline/driver.rs:3996-4027). Before that rekey the round has already landed under the void handle space:

- a new UID equal to an old one merges flags onto the wrong item, and a pending local flag is pushed to the wrong message;
- every old binding the round did not list is dropped `Deleted` at its last page, and in a two-endpoint account the other source gets a `Tombstone` for each;
- when the rekey then fails, the next run sees no epoch change and never rekeys, while the other endpoint pushes `Remove` for every such message.

pimdir SYNC §4 already says the opposite: a connector whose checkpoint the source rejects because the handle space changed MUST fail the enumeration and name a rekey (§8), never answer a round of new handles; a head meeting a moved handle space fails it the same way. This repository's own spec encodes the wrong behaviour (cairn/spec/sync.md, "An IMAP handle-space change rebuilds the collection" and "IMAP enumeration is incremental").

## What

- The IMAP connector compares the `UIDVALIDITY` a `SELECT` answers with the one the store's handles were bound under: the stored checkpoint's, else the open round's. On a mismatch every listing path answers `Listed::Rekey`: a QRESYNC delta, a delta on a server without QRESYNC, a delta head, a band head, a round's first page and a resumed round's page. A round's cursor naming another `UIDVALIDITY` than the server's, the store already holding the server's, still restarts the round.
- `Listed::Rekey` fails the enumeration: the remote records it and errors, the sync verb fails before any write of that page, and the driver runs io-pimdir's `PimdirRekey` on the collection, then syncs it again. A rekey that fails leaves the stored checkpoint untouched, so the next run meets the moved `UIDVALIDITY` again and rekeys then: the rekey is retried until it lands, and nothing is listed under the void handles meanwhile.
- The post-sync epoch comparison goes, the connector naming the rekey before anything lands.
- The open round's checkpoint stands for the store's while no round has closed, so a first round resumed across a `UIDVALIDITY` change rekeys too.

## Not done

- A pimdir vector for a moved handle space run through neverest's seam: the vectors run against the io-pimdir core only (report-neverest.md, missing tests).
