---
cairn: change
id: a-graph-pass-states-no-guessed-removal
status: landed
created: 2026-11-13
---

# A Graph pass states no guessed removal

## Why

pimdir audit line Y1 (pimdir cairn/changes/audit-2026-11-12/tasks.md): the pass closing a scoped Graph round handed as `vanished` every bound handle its fresh delta did not list. pimdir SYNC §4 forbids it: a connector states a removal only for what its source states removed, never for what a listing failed to return. A stated removal is applied on any page, whatever the date, and no guard counts it, so a delta answered short dropped every unlisted binding and a two-endpoint account pushed the deletes.

## What

Two ways out were on the table (cairn/log/2026-11-13-neverest-follows-draft-10.md):

1. Take the delta link before the band pages and follow it at the round's end, so Graph itself states every removal of the round. It needs a link made without enumerating the folder, `$deltatoken=latest`.
2. State only the rows Graph marks removed, and accept that a message a band page listed and Graph dropped before the pass stays until a later round.

Microsoft's documentation decides for the second. "Use delta query to track changes in Microsoft Graph data" (learn.microsoft.com/en-us/graph/delta-query-overview, read 2026-11-13) describes `$deltatoken=latest` for Microsoft Entra ID resources, and `token=latest` for OneDrive and SharePoint; "message: delta" (learn.microsoft.com/en-us/graph/api/message-delta) and "Get incremental changes to messages in a folder" (learn.microsoft.com/en-us/graph/delta-query-messages) list `$deltatoken` and `$skiptoken` as state tokens of a previous call and say nothing of `latest`. Nothing documented says a mail folder's delta takes it, so neverest does not rely on it.

- The pass states gone only a row its delta marks `@removed`.
- A member it does not list is the engine's to find absent, from the ids the pass hands as its unnarrowed enumeration, in the round's scope and under the guards.
- The limit is written into the spec: the stamped row of a message dropped between its band page and the pass stays until a later round over its scope.
