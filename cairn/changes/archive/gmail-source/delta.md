---
cairn: delta
change: gmail-source
---

## ADDED Requirements

### Requirement: Gmail is a native mail source
A `gmail` source SHALL open protocol-direct over io-gmail and sync `message/rfc822`. Its collections SHALL be the user labels and the `INBOX`, `SENT`, `DRAFT`, `SPAM` and `TRASH` system labels, keyed and named by label name, as IMAP exposes them, so a Gmail endpoint and an IMAP endpoint on one account pair their labels. A label id SHALL NOT key a collection: the driver pairs endpoints and creates missing collections by key, and an opaque id would never meet the IMAP name.

`UNREAD`, `STARRED` and `IMPORTANT` SHALL be flags (`\Seen` by absence, `\Flagged`, `$Important`), never collections, and the categories SHALL NOT be collections. Any other flag SHALL be dropped on push, a keyword turned label filing the message in a new collection.

A handle SHALL be the Gmail message id, stable across labels. Enumeration SHALL carry the mailbox `historyId` as the checkpoint, taken before a full round's first list. A delta SHALL be `history.list` scoped to the collection's label, each touched message's current labels read before it is reported, and an expired `historyId` (404) SHALL restart a full round. A spam or trashed message SHALL be a member of `SPAM` or `TRASH` only.

A delete from a label SHALL remove that label (from `INBOX` it archives), a move SHALL swap two labels in one modify, and only a delete from `TRASH` SHALL delete permanently. An added message SHALL go through `messages.import`, and an answered id that does not exist SHALL be re-found through history by `Message-ID` rather than trusted. A system label SHALL NOT be created or deleted.

Auth SHALL be a bearer access token only, as for every other HTTP source.

## MODIFIED Requirements

### Requirement: Sources are remote backends only
Gmail joins IMAP and Microsoft Graph for `message/rfc822`; the parenthetical "JMAP and Gmail as their backends land" keeps JMAP only.

### Requirement: Every remote backend is a cargo feature
`gmail` gates the Gmail backend.

### Requirement: A collection is keyed by its backend id and named by its display name
The label name keys a Gmail collection, next to the mailbox name on IMAP and Graph.

## REMOVED Requirements
