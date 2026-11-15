---
cairn: delta
change: a-gmail-keyword-is-refused-up-front
---

# Delta

## ADDED Requirements

None.

## MODIFIED Requirements

### Requirement: Gmail is a native mail source
A `gmail` source SHALL open protocol-direct over io-gmail and sync `message/rfc822`. Its collections SHALL be the user labels and the `INBOX`, `SENT`, `DRAFT`, `SPAM` and `TRASH` system labels, keyed and named by label name, as IMAP exposes them, so a Gmail endpoint and an IMAP endpoint on one account pair their labels. A label id SHALL NOT key a collection: the driver pairs endpoints and creates missing collections by key, and an opaque id would never meet the IMAP name.

`UNREAD`, `STARRED` and `IMPORTANT` SHALL be flags (`\Seen` by absence, `\Flagged`, `$Important`), never collections, and the categories SHALL NOT be collections. Any other flag SHALL be dropped on push, a keyword turned label filing the message in a new collection. A Gmail source SHALL declare `mail.flags.important` `full` and `mail.flags.junk` `none` (spam being the `SPAM` collection, reached by a move), so a store reads `$Important` and `$Junk` under their own rows (pimdir STORAGE Annex B.1), and `mail.flags.keywords` `none`, naming `$Important` as the one keyword kept: a frontend's change of any other keyword on a Gmail label, a `set-flags` or an `add`, `copy` or `move` landing a message carrying one, SHALL be refused at the store's gate rather than queued and dropped on push, and one queued before SHALL be parked by the drain. Every reader of a store at this version reads the two own rows (io-pimdir 0.9 on), so `$Important` needs nothing of `mail.flags.keywords`. Other sources state neither row, their two flags read under `mail.flags.keywords`.

A handle SHALL be the Gmail message id, stable across labels. Enumeration SHALL carry the mailbox `historyId` as the checkpoint, taken before a full round's first list. A delta SHALL be the mailbox history, read once a run with no label and projected on the collection's label (a record naming the label, a message the collection holds, an arrival naming no label), each touched message's current labels read before it is reported, and an expired `historyId` (404) SHALL restart a full round. A spam or trashed message SHALL be a member of `SPAM` or `TRASH` only.

While a label's round is open, a run resuming it SHALL list the round's delta head before its next page (pimdir SYNC §4, §5): the mailbox history since the round's history id, read once a run and projected on the label as a delta is, answered as a delta with the history id Gmail holds now; an expired history id (404) or a checkpoint that is no history id SHALL reject the head, which restarts the round; a band head SHALL be answered none. A page of the label listed after its head SHALL read every message the head touched (read again or stated deleted) no earlier than the head, the run's own read of it since the head serving, its labels then deciding its membership and flags; flag listings read before the head SHALL be read again. A message a page reads that no longer carries the label SHALL be left out, as one gone (404) is, a round page stating no removal.

A delete from a label SHALL remove that label (from `INBOX` it archives), a move SHALL swap two labels in one modify, and only a delete from `TRASH` SHALL delete permanently. A write on a message gone already (404) SHALL count as done. An added message SHALL go through `messages.import`, and an answered id that does not exist SHALL be re-found through history by `Message-ID` rather than trusted. A system label SHALL NOT be created or deleted.

Auth SHALL be a bearer access token only, as for every other HTTP source.

#### Scenario: A message arriving while the Inbox's round is open
- **GIVEN** a Gmail Inbox whose round's first page landed from history id 500, the round left open
- **WHEN** a message arrives in the Inbox and the account is synced again
- **THEN** the run lists the history since 500 first, the message lands before the round's next page, and the round closes on the history id that head read, which the next delta lists from

#### Scenario: A keyword set on a Gmail message
- **GIVEN** a store holding a Gmail message, its source declared
- **WHEN** a frontend queues `$Custom` on it, then `$Important`
- **THEN** `$Custom` is refused at the gate naming `mail.flags.keywords` and Gmail's detail, and `$Important` is queued with no partial support to show

## REMOVED Requirements

None.
