---
cairn: change
change: a-moved-uidvalidity-rekeys-first
---

# Delta

## ADDED Requirements

None.

## MODIFIED Requirements

### Requirement: An IMAP handle-space change rebuilds the collection and bumps its generation
An IMAP listing SHALL compare the `UIDVALIDITY` its `SELECT` answers with the one the store's handles were bound under: the stored checkpoint's, else, while no round has closed, the open round's. On a mismatch it SHALL fail the enumeration and name a rekey (pimdir SYNC §4, §8), whatever it was asked for (a delta, a head, a round's first page or a resumed page), never answering a round of the new UIDs: a sync reads such a round as every member gone and every new one arrived, merges flags onto whatever an old UID now names, and tombstones every message on another endpoint.

The driver SHALL then run io-pimdir's rekey on the collection before syncing it again, carrying cached bodies, summaries and pending state over by link id. The rebuild batch drops every old handle as `Rekeyed`, which the store reads as the rebuild and answers by bumping `collections.generation` in the transaction applying it (pimdir SYNC §8), so a frontend derives its epoch from the store alone. A rekey that fails leaves the stored checkpoint as it was, so the next run meets the moved `UIDVALIDITY` again and rekeys then. Ordinary syncs and full resyncs never bump. Graph sources never rebuild: Graph message ids survive a delta reset.

#### Scenario: A mailbox renumbered between two runs
- **GIVEN** a mailbox synced under one `UIDVALIDITY`, one of its messages carrying a flag staged in the store
- **WHEN** the server renumbers the mailbox and the account is synced
- **THEN** the message is bound under its new UID with the staged flag, nothing is dropped as deleted, and the flag is pushed to the new UID

### Requirement: IMAP enumeration is incremental (QRESYNC)
Enumeration SHALL carry a per-mailbox checkpoint `(UIDVALIDITY, HIGHESTMODSEQ)`. On a QRESYNC-capable server (ENABLEd on connect) with a checkpoint whose UIDVALIDITY still matches, a delta SHALL be a QRESYNC `SELECT (QRESYNC (uidvalidity highestmodseq))`: only the messages changed since the modseq, the meta read for the ones the store does not bind, plus the vanished UIDs, issuing **no FETCH when nothing changed**. Without a usable checkpoint (first sync, malformed bytes) or on a non-QRESYNC server it SHALL answer a round, page by page. A checkpoint whose UIDVALIDITY moved SHALL fail the delta and name a rekey, never answer a round.

## REMOVED Requirements

None.
