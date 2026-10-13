---
cairn: change
change: an-open-imap-round-lists-its-head
---

# Delta

## ADDED Requirements

### Requirement: An open IMAP round lists its head
An IMAP source SHALL state delta heads and band heads where its server advertises QRESYNC, band heads alone elsewhere, CONDSTORE without QRESYNC stating no removal, read from the capabilities its connection read when it opened (pimdir SYNC §4); the sync SHALL tell the engine the heads its source states. A delta head SHALL be the QRESYNC `SELECT` from the checkpoint asked, answered as a delta and never as a round page: the messages changed since, their meta read where the store does not bind them under that `UIDVALIDITY`, the vanished UIDs, and the checkpoint the server holds now. A server without QRESYNC, or a checkpoint with no `UIDVALIDITY` or no modseq, SHALL list none; a checkpoint whose `UIDVALIDITY` moved SHALL be refused, the round restarting and its close rebuilding the collection. A band head SHALL be `UID SEARCH SINCE` the day the round opened less two days, by the `INTERNALDATE`, each message's flags or meta read as a round page reads them, with no removal and no checkpoint; an instant it cannot read lists none. A page SHALL read its members by `UID FETCH` when it lists, leaving out a UID the mailbox no longer holds. The last page of a round resumed from a cursor SHALL list again, read after its `SELECT`, every message of the scope above the highest UID the round found when it opened, so what a band head named, stamping nothing, is not found absent when the round closes; a cursor written before it carried that UID SHALL list the whole scope again.

#### Scenario: An arrival and an expunge while a round is open
- **GIVEN** a QRESYNC mailbox of 1,100 messages whose first sync was killed after its first page, then a message appended and one the first page listed expunged
- **WHEN** the account is synced again
- **THEN** the arrival lands and the expunged message goes before any further page, the round still open; the round then closes keeping the arrival, and the next delta lists nothing new

#### Scenario: A band head's arrival survives the close
- **GIVEN** the same mailbox read as a server without QRESYNC
- **WHEN** the account is synced again
- **THEN** a band head lands the arrival while the round is open, the round's close finds nothing absent, and the next round drops the expunged message

## MODIFIED Requirements

### Requirement: A mail round lands page by page, newest first
A mail round SHALL be answered in pages, newest first, each landing in its own write with its resume cursor: IMAP 500 UIDs by UID descending, its cursor `(UIDVALIDITY, lowest UID listed, highest UID found when the round opened)`; Graph 1,000 messages a page, from the message delta with no scope (`Prefer: odata.maxpagesize`, sent with every link) or from `/messages` by `sentDateTime` under one (`$top`), its cursor the next link; Gmail 100 ids, its cursor the page token. The checkpoint SHALL be the one taken when the round began where the provider gives one (IMAP `HIGHESTMODSEQ` at the select, Gmail's profile `historyId` read before the listing), Graph's delta link on its last page. An interrupted round SHALL resume from its cursor, and a cursor the provider refuses (an expired Graph link, a Gmail page token, a moved `UIDVALIDITY`) SHALL restart it.

#### Scenario: A round killed after its first page
- **GIVEN** a mailbox of 1,100 messages synced for the first time
- **WHEN** the run is killed once a page landed, and the account is synced again
- **THEN** the second run closes the round and the store holds every message once

## REMOVED Requirements

None.
