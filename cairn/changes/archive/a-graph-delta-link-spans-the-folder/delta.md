---
cairn: change
change: a-graph-delta-link-spans-the-folder
---

# Delta

## ADDED Requirements

### Requirement: A Graph mail folder keeps one delta link over the whole folder
A Graph mail source's checkpoint SHALL be a message delta link made over the whole folder, with no date filter, whatever the scope, and the source SHALL be bound to no scope (pimdir SYNC §4). With no scope, a round SHALL be that delta, with the summary `$select`, its last page carrying the link. Under a scope, a round SHALL list the scope's mail by its `sentDateTime` through `/messages` (`$filter=sentDateTime ge A and sentDateTime lt B`, `B` left open for the newest band, `$orderby=sentDateTime desc`, the summary `$select`, `$top` 1,000), then pass once over the folder's delta with `$select=id,sentDateTime,isRead,isDraft,flag`, `Prefer: odata.maxpagesize=1000` on every request, as its last page: the pass SHALL list every member in scope, by its binding, by the summary the band read, else by one read in JSON batches, SHALL state gone every member the store binds or the band listed that it does not list, and SHALL carry the link. A pass interrupted SHALL start again. A round over the band a coverage lacks SHALL list that band alone and keep the link, its last page listing the messages with no date the store binds, by their last-synced flags.

A delta SHALL follow the link: a change whose `sentDateTime` lies outside the scope SHALL be dropped, a removal SHALL apply whatever the date, a member the store binds SHALL be listed by its flags, and a member in scope it does not bind SHALL be named by its summary, the row's own on a link made with summaries, else read in JSON batches of 20, a message gone since being left out and any other summary Graph does not answer failing the listing. The link SHALL serve every later scope: a narrower one follows it, a wider one lists its band; only an expired link (410) or a full sync SHALL open a round. The checkpoint SHALL name what the link's rows carry; a link stored before it did SHALL be refused when the coverage is bounded, a `$filter` having made it.

#### Scenario: Three widenings
- **GIVEN** a Graph folder synced under `--since 2026-09-01`
- **WHEN** it is synced under `--since 2026-07-01`, then `2026-06-01`, then `2026-03-01`
- **THEN** each run lists only the messages of the band it adds, the folder is passed over once in all, and the checkpoint is the link the first round made

#### Scenario: A change out of scope
- **GIVEN** that folder synced under `--since 2026-07-01`
- **WHEN** a message of March is flagged on the server, one of September is flagged, and two messages arrive, one dated today and one dated 2020, and the folder is synced again
- **THEN** the September flag reaches the store, the March one does not, and only the message dated today is read and stored

## MODIFIED Requirements

### Requirement: A provider narrows a scope with a margin
A connector MAY narrow a scoped listing by its provider's own filter to a superset of the scope: IMAP `SENTSINCE` a day below the floor (or no `Date` header) and `SENTBEFORE` two days above a ceiling, Gmail `after:` and `before:` two days around the scope, in epoch seconds. Graph lists by `sentDateTime`, its reading of the `Date` header, with no margin. The `Date` SHALL decide what is kept. A connector whose checkpoint is bound to no scope (IMAP, Gmail, Graph mail) SHALL widen a scope by listing only the band its coverage lacks.

### Requirement: A mail round lands page by page, newest first
A mail round SHALL be answered in pages, newest first, each landing in its own write with its resume cursor: IMAP 500 UIDs by UID descending, its cursor `(UIDVALIDITY, lowest UID listed)`; Graph 1,000 messages a page, from the message delta with no scope (`Prefer: odata.maxpagesize`, sent with every link) or from `/messages` by `sentDateTime` under one (`$top`), its cursor the next link; Gmail 100 ids, its cursor the page token. The checkpoint SHALL be the one taken when the round began where the provider gives one (IMAP `HIGHESTMODSEQ` at the select, Gmail's profile `historyId` read before the listing), Graph's delta link on its last page. An interrupted round SHALL resume from its cursor, and a cursor the provider refuses (an expired Graph link, a Gmail page token, a moved `UIDVALIDITY`) SHALL restart it.

#### Scenario: A round killed after its first page
- **GIVEN** a mailbox of 1,100 messages synced for the first time
- **WHEN** the run is killed once a page landed, and the account is synced again
- **THEN** the second run closes the round and the store holds every message once

## REMOVED Requirements
