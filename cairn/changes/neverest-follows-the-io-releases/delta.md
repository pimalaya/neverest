---
cairn: delta
change: neverest-follows-the-io-releases
---

# Delta

## ADDED Requirements

### Requirement: An IMAP listing is read whole or fails
An answer a listing is read from (a `UID SEARCH`, and the `UID FETCH` of a page's flags or header fields) holding a line io-imap could not decode SHALL fail the listing, naming the command and the count of such lines. The line may be a row, and a UID a page leaves out is a message the mailbox no longer holds: read without it, the page states the message removed. The search looking for a message by its `Message-ID` before a copy or a removal SHALL fail the same way.

The search naming the message an `APPEND` just delivered is no listing: the append is accepted whatever it answers. It, and a `SELECT`, `LIST` or `STATUS` answer holding such a line, SHALL be read with a warning naming the count. The text of an `ALERT` response code SHALL be shown as a warning.

#### Scenario: A row that does not decode
- GIVEN a mailbox one of whose messages carries a flag that is no IMAP atom
- WHEN its page is listed
- THEN the listing fails, naming the `FETCH` answer, and no message is stated removed

### Requirement: A DAV answer that is no whole multistatus fails
A `207` answer whose body is not one whole `multistatus` document (a gateway's page, a document cut short or malformed, an empty body) SHALL fail the request reading it, as io-webdav refuses it: a `sync-collection` report, the `PROPFIND` listing standing in for it, a collection listing, a multiget. It SHALL NOT be read as a listing of what parsed, which a complete listing turns into the removal of every member left out. A failed listing is reported as "A collection that failed to scan is reported, never only logged" says, and a failed multiget falls back as "A body fetch answers for every handle it was asked about" says.

A `DELETE` or a `MOVE` answered `207` naming a member with a status other than 2xx (RFC 4918 §9.6.1) SHALL fail, the member being still there.

#### Scenario: A gateway answers a listing with its own page
- GIVEN a CardDAV server whose `sync-collection` report is answered `207` with an HTML page
- WHEN the account is synced
- THEN the address book fails to list, and no card is stated removed

## MODIFIED Requirements

### Requirement: A body fetch answers for every handle it was asked about
A paragraph and two scenarios are added. A batch that errors SHALL be read item by item on a connection fit for it. An IMAP batch the server refused with a tagged `NO` or `BAD` was answered whole, and its items SHALL be read on the connection it failed on. After any other failure (the connection lost, a body that could not be written, an answer the client stopped reading), what is left of the answer may still be on the connection, where the next command would read it as its own: the connection SHALL be dropped and opened again before any item is read, and one that does not open again SHALL fail the batch.

#### Scenario: An IMAP batch failing in the middle of its answer
- GIVEN a batched fetch of two messages whose answer the client stops reading at its second row
- WHEN the run continues
- THEN a new connection is opened, the two messages are read one by one on it, and each handle holds the body of its own message

#### Scenario: An IMAP batch the server refuses
- GIVEN a batched fetch the server answers with a tagged `NO`
- WHEN the run continues
- THEN its messages are read one by one on the same connection

### Requirement: A queued submission is a `submit` queue intent
The sentence "A refusal SHALL reset the SMTP transaction before the next message; a refusal for a sending rate or quota, or a failure with no reply or status, SHALL leave the rest of the run's intents unattempted, pending." is replaced. A refused recipient or a refused `DATA` command SHALL leave the SMTP transaction reset before the next message, one `RSET` sent by io-smtp; a refused `MAIL FROM` opens no transaction and a refused body ends its own, so neither is followed by a reset. A refusal for a sending rate or quota, a `421` reply, by which the server closes the session (RFC 5321 §4.2.2), or a failure with no reply or status, SHALL leave the rest of the run's intents unattempted, pending. An envelope address io-smtp refuses, one that is no mailbox or holds a line break, SHALL fail its intent as permanent before anything is sent.

### Requirement: A send that may have been delivered is never sent again
Over SMTP a `submit` is unconfirmed once the data's final `.` was written and flushed and no reply read, the `RSET` following a refused `DATA` command being no data.

### Requirement: Google contacts and calendars are native sources
The sentence on an update is replaced. An update SHALL carry the etag, which People enforces, and SHALL merge back, in each field of its mask, what People holds there and a vCard cannot show (io-gpeople's `merge_unprojected`: an organization or occupation after the first, a relation other than spouse or child, a custom type, a sub-field without a vCard slot, the `clientData` entries other clients own), a masked update replacing the field whole. A property too long for the stash, which the projection leaves out, SHALL be named in a warning.

## REMOVED Requirements
