---
cairn: log
change: a-gmail-round-lists-its-head
date: 2026-10-13
---

# A Gmail round lists its head

A large Gmail Inbox keeps its first round open for many runs, paced by the quota, and a message arriving meanwhile showed only once the round closed. A run resuming the round now lists its head first (pimdir `round-head`, io-pimdir `07bd316`): the history since the round's history id, so the arrival lands before the round's next page and the round closes on the history id the head read.

## What landed

- src/client.rs: `Client::heads` answers `PimdirHeads::DELTA` for Gmail.
- src/gmail/client.rs: `delta_head`, the delta from the round's history id over the run's history, rejected on a 404 or a checkpoint that is no history id, a band head answered none; round pages read what a head touched no earlier than the head, leave out a message read without the label, and read flag listings again after a head; scripted tests.
- src/gmail/run.rs: `Headed`, what each label's head touched and when it began.
- src/offline/driver.rs: `sync_side` passes the source's heads to the sync verb (none was passed, so no head was ever asked); driver tests over scripted Gmail sessions (vector 62's case, a deletion in a head, an expired head).
- src/offline/mod.rs: a head opens a resumed round for the pager.

## Left

- A live run on the test account.
- IMAP and Graph heads (their own changes).

## Capabilities moved

**sync**: the Gmail source requirement (heads, freshness of later pages), priming (a resumed round lists its head before it stops).
