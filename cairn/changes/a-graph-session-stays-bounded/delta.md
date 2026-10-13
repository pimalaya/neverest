---
cairn: change
change: a-graph-session-stays-bounded
---

# Delta

## ADDED Requirements

### Requirement: A Graph connection is reused only after a whole answer
A Graph session SHALL send its next request on the same connection only after an exchange that read a whole answer on a connection the server neither ended nor asked to close, and SHALL reopen a connection idle for more than 60 seconds, below the two minutes after which Graph closes one. Any other failure (a read that timed out, a reset, an answer cut short) SHALL drop the connection, so a late answer is never read as the next request's. A request that only reads (a `GET`, a `$batch` of gets) that met a reused connection already dead before any octet of its answer (reset, aborted, broken pipe, end of stream) SHALL be sent again once, on a new connection (RFC 9112 §9.3.1); a read that timed out SHALL NOT be sent again, nor SHALL a request that creates, sends or moves.

#### Scenario: A connection closed while it idled
- **GIVEN** a Graph session whose connection the server closed after its last answer
- **WHEN** the session asks for the next page of a listing
- **THEN** the page is asked again on a new connection and the listing goes on

#### Scenario: A move on a dead connection
- **GIVEN** that session
- **WHEN** it pushes a move
- **THEN** the move fails without being sent again, and the next request opens a new connection

### Requirement: A Graph write on an item gone counts as done
A flag set or a delete on a Graph message answered 404 SHALL count as done, as SHALL the delete of a Graph contact or event answered 404 on the delete or on the read that checks its revision. A move answered 404 SHALL count as done only when the message reads as gone by its id; still there, the 404 names the destination, and the move SHALL fail. A copy of a message gone SHALL fail.

#### Scenario: A message deleted on the server meanwhile
- **GIVEN** a message whose `\Seen` is staged in the store and that was deleted on Microsoft 365 since the last sync
- **WHEN** the account syncs
- **THEN** the flag push counts as done, the run is complete, and the message is gone from the store

### Requirement: A Graph listing keeps a few octets a message
A Graph mail delta and the pass SHALL be read page by page, each page dropped once read, keeping for each message its id and what its last row says: removed, out of scope, its flags, or, on a link made with summaries, the summary of a member in scope the store does not bind. A session SHALL keep no summary from one listing to the next: a band round followed by a pass SHALL keep the flags of what it listed, three bits a message, dropped by the pass, and the `Meta` tier SHALL read each summary by its id.

## MODIFIED Requirements

### Requirement: Graph mail bodies are fetched in JSON batches
A Graph mail source SHALL fetch the `Full` tier of an id set through JSON batches (`POST /$batch`) of raw MIME gets (`/messages/{id}/$value`), each body answered base64-encoded, matched to its message by request id whatever order Graph answers in, and written to the blob store as soon as it is decoded, before the next is read. A batch SHALL hold at most 20 gets and 4 MiB of the sizes Graph states for them (the MAPI message size, `$expand=singleValueExtendedProperties($filter=id eq 'Integer 0x0E08')`, which the band listing, the summaries read by id and the whole-folder delta ask for, the delta asking without it once Graph refused it); a message stated over 1 MiB, or stating no size, SHALL be read alone by a plain get. When more than one message of a fetch states no size to the session, their sizes SHALL be read first, 20 a batch, by id alone. A get Graph throttles (429 or a common 5xx), alone or with its whole batch, SHALL be sent again in a later batch once its own `Retry-After` is over (seconds or an HTTP date), else after a back-off doubling from a second, at most four times; one still throttled, or stating a wait over two minutes, SHALL be left unanswered, a throttle (429, 503) giving the source up until its wait is over. A get refused otherwise SHALL be left unanswered, so the per-item fallback fetches it and surfaces its error.

#### Scenario: Twenty bodies, one round trip
- **GIVEN** a Graph mail folder holding twenty messages of 50 KiB each whose bodies the store lacks
- **WHEN** a sync hydrates them
- **THEN** one batch request fetches all twenty, stored byte for byte as a direct get would

#### Scenario: Large mail is read alone
- **GIVEN** that folder with three messages of 2 MiB more
- **WHEN** a sync hydrates them
- **THEN** each of the three is read by its own get, and no batch asks for more than 4 MiB

### Requirement: A Graph mail folder keeps one delta link over the whole folder
(Only the pass sentence changes; the rest stands.) … then pass once over the folder's delta with `$select=id,sentDateTime,isRead,isDraft,flag`, `Prefer: odata.maxpagesize=1000` on every request, as its last page: the pass SHALL list every member in scope the store binds by its flags, SHALL list again a member the band listed in the same run only when its flags moved since, by one read, and every other member in scope by one read in JSON batches, SHALL state gone every member the store binds or the band listed that it does not list, and SHALL carry the link. …

### Requirement: Microsoft Graph is a first-class source
(Only the mail paragraph's `Meta` tier changes.) … the `Meta` tier read by one summary get a message (`mid:`/`alt:` link ids, meta v1), …

### Requirement: Every listed member arrives named
(Only Graph's clause changes.) … Graph reads its summary `$select` and the MAPI size `$expand`; …

## REMOVED Requirements
