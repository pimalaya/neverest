---
cairn: change
change: a-gmail-round-lists-its-head
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: Gmail is a native mail source
A `gmail` source SHALL open protocol-direct over io-gmail and sync `message/rfc822`. Its collections SHALL be the user labels and the `INBOX`, `SENT`, `DRAFT`, `SPAM` and `TRASH` system labels, keyed and named by label name, as IMAP exposes them, so a Gmail endpoint and an IMAP endpoint on one account pair their labels. A label id SHALL NOT key a collection: the driver pairs endpoints and creates missing collections by key, and an opaque id would never meet the IMAP name.

`UNREAD`, `STARRED` and `IMPORTANT` SHALL be flags (`\Seen` by absence, `\Flagged`, `$Important`), never collections, and the categories SHALL NOT be collections. Any other flag SHALL be dropped on push, a keyword turned label filing the message in a new collection.

A handle SHALL be the Gmail message id, stable across labels. Enumeration SHALL carry the mailbox `historyId` as the checkpoint, taken before a full round's first list. A delta SHALL be the mailbox history, read once a run with no label and projected on the collection's label (a record naming the label, a message the collection holds, an arrival naming no label), each touched message's current labels read before it is reported, and an expired `historyId` (404) SHALL restart a full round. A spam or trashed message SHALL be a member of `SPAM` or `TRASH` only.

While a label's round is open, a run resuming it SHALL list the round's delta head before its next page (pimdir SYNC §4, §5): the mailbox history since the round's history id, read once a run and projected on the label as a delta is, answered as a delta with the history id Gmail holds now; an expired history id (404) or a checkpoint that is no history id SHALL reject the head, which restarts the round; a band head SHALL be answered none. A page of the label listed after its head SHALL read every message the head touched (read again or stated deleted) no earlier than the head, the run's own read of it since the head serving, its labels then deciding its membership and flags; flag listings read before the head SHALL be read again. A message a page reads that no longer carries the label SHALL be left out, as one gone (404) is, a round page stating no removal.

A delete from a label SHALL remove that label (from `INBOX` it archives), a move SHALL swap two labels in one modify, and only a delete from `TRASH` SHALL delete permanently. A write on a message gone already (404) SHALL count as done. An added message SHALL go through `messages.import`, and an answered id that does not exist SHALL be re-found through history by `Message-ID` rather than trusted. A system label SHALL NOT be created or deleted.

Auth SHALL be a bearer access token only, as for every other HTTP source.

#### Scenario: A message arriving while the Inbox's round is open
- **GIVEN** a Gmail Inbox whose round's first page landed from history id 500, the round left open
- **WHEN** a message arrives in the Inbox and the account is synced again
- **THEN** the run lists the history since 500 first, the message lands before the round's next page, and the round closes on the history id that head read, which the next delta lists from


### Requirement: Every collection's newest mail comes first on request
`sync --prime <N>` SHALL, under a positive N, run phase 1 in two passes. The first SHALL take every collection in the run's order: a round SHALL list its first page and stop at rest before asking its second, the store left as a run killed there leaves it (the round open at its cursor); a round an earlier run left open SHALL list its head where its source lists one, and stop before asking its next page; a collection whose rounds are closed SHALL run its delta to the end. Right after, the collection's N newest bodies still owed (by the mail summary's date) SHALL download ahead of any other work, on whichever connection is free, the others it owes joining the feed of the idle connections. The second pass SHALL start once every first-pass spine and every primed body has ended, and SHALL list each stopped collection to its end, in the run's order, from its cursor; its pushes, its staged work and its coverage are done then, as for a collection listed whole. Phase 2 SHALL download each collection's N newest owed bodies ahead of every other, then the rest in the download order. The report SHALL name each fetch once, a stopped spine handing the next one what it owed. The default SHALL be 50 under `--download-order newest` and 0 under `largest`; `--prime 0` lists each collection whole in turn; a dry run never primes. Priming orders a run and SHALL NOT shorten it: a run that ends has listed every page and downloaded every owed body.

#### Scenario: One connection, three large folders
- **GIVEN** three folders of 1,100 mails each, synced for the first time over one connection under `--download-order newest`
- **WHEN** the third folder's first page lands
- **THEN** the first two hold their first page alone, and each folder's 50 newest mails are readable before any second page lists

#### Scenario: Killed once primed
- **GIVEN** that run killed once each folder's 50 newest mails are readable
- **WHEN** the next run syncs the account
- **THEN** every round closes and each folder holds each of its mails once, with its body


## REMOVED Requirements
