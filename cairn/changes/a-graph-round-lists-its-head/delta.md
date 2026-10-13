---
cairn: change
change: a-graph-round-lists-its-head
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: A Graph mail folder keeps one delta link over the whole folder
A Graph mail source's checkpoint SHALL be a message delta link made over the whole folder, with no date filter, whatever the scope, and the source SHALL be bound to no scope (pimdir SYNC §4). With no scope, a round SHALL be that delta, with the summary `$select`, its last page carrying the link. Under a scope, a round SHALL list the scope's mail by its `sentDateTime` through `/messages` (`$filter=sentDateTime ge A and sentDateTime lt B`, `B` left open for the newest band, `$orderby=sentDateTime desc`, the summary `$select`, `$top` 1,000), then pass once over the folder's delta with `$select=id,sentDateTime,isRead,isDraft,flag`, `Prefer: odata.maxpagesize=1000` on every request, as its last page: the pass SHALL list every member in scope the store binds by its flags, SHALL list again a member the band listed in the same run only when its flags moved since, by one read, and every other member in scope by one read in JSON batches, SHALL state gone every member the store binds or the band listed that it does not list, and SHALL carry the link. A pass interrupted SHALL start again. A round over the band a coverage lacks SHALL list that band alone and keep the link; the messages with no date, which a `sentDateTime` filter never lists, SHALL stay bound, the band round inferring no delete of them.

A delta SHALL follow the link: a change whose `sentDateTime` lies outside the scope SHALL be dropped, a removal SHALL apply whatever the date, a member the store binds SHALL be listed by its flags, and a member in scope it does not bind SHALL be named by its summary, the row's own on a link made with summaries, else read in JSON batches of 20, a message gone since being left out and any other summary Graph does not answer failing the listing. The link SHALL serve every later scope: a narrower one follows it, a wider one lists its band; only an expired link (410) or a full sync SHALL open a round. The checkpoint SHALL name what the link's rows carry; a link stored before it did SHALL be refused when the coverage is bounded, a `$filter` having made it.

While a folder's round is open, a run resuming it SHALL list the round's band head before its next page (pimdir SYNC §4, §5): the folder's messages Graph received from two days before the instant the round opened, through `/messages` (`$filter=receivedDateTime ge T`, `$orderby=receivedDateTime desc`, the summary `$select`, the size `$expand` until Graph refuses it with a 400, `$top` 512, every next link followed), checked against the run's scope by `sentDateTime`, a member the store binds listed by its flags and any other by its summary, with no checkpoint and no removal. A delta head SHALL be answered none, the link arriving with a round's last page only, as SHALL a band head whose instant reads as none. A run that listed a band head SHALL list it again in the round's last page, read after that page's own rows: a whole-folder round's last delta page and a band round's last band page SHALL add every member in the round's scope that listing names and the page neither lists nor states removed; under a scope, the pass SHALL read the summary of a member the head named this run as of any member the band did not list, listing every member in scope still in the folder.

#### Scenario: Three widenings
- **GIVEN** a Graph folder synced under `--since 2026-09-01`
- **WHEN** it is synced under `--since 2026-07-01`, then `2026-06-01`, then `2026-03-01`
- **THEN** each run lists only the messages of the band it adds, the folder is passed over once in all, and the checkpoint is the link the first round made

#### Scenario: A change out of scope
- **GIVEN** that folder synced under `--since 2026-07-01`
- **WHEN** a message of March is flagged on the server, one of September is flagged, and two messages arrive, one dated today and one dated 2020, and the folder is synced again
- **THEN** the September flag reaches the store, the March one does not, and only the message dated today is read and stored

#### Scenario: A message arriving while the folder's first round is open
- **GIVEN** a Graph Inbox synced with no scope whose round's first page landed, the round left open
- **WHEN** two messages arrive, the account is synced and stopped after the head, one of them is deleted, and the account is synced again
- **THEN** both land before the round's next page, the round's last page lists the one still there again after the delta link, and once the round closes the store holds it and not the other


## REMOVED Requirements
