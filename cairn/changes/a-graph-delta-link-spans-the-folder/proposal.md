---
cairn: change
id: a-graph-delta-link-spans-the-folder
status: landed
created: 2026-10-07
---

# A Graph delta link spans the whole folder

## Why

`scoped-mail-sync` listed a Graph mail folder under a scope through a message delta filtered by `receivedDateTime ge since−2d`, and declared the link bound to its scope. Graph's delta takes no upper bound on any date, and a filtered link reports the changes of its filter alone, so every widening of `item.filter.since` opened a round over the whole wider scope: filling a folder of `n` mails in steps of 500 relisted about `n²/1000` mails. MOA widens a box's scope step by step to make the first sync usable in minutes, so on Microsoft 365 each step cost more than the last.

## What

Decided by Clément DOUIN, the same design as the Android bridge:

- **One delta link per folder, with no date filter.** With no scope (MOA's default, whole folders), a round stays the message delta with the summary `$select`, `maxpagesize` 1,000, the link its checkpoint. Under a scope, the link is made by one pass of the folder's delta with `$select=id,sentDateTime,isRead,isDraft,flag` (the date the scope reads and what the flags map from), `maxpagesize` 1,000 on every request.
- **Mail listed by date band.** Under a scope, a round lists the scope's mail through `/messages` with `$filter=sentDateTime ge A and sentDateTime lt B` (exact on the `Date` header, `B` left open for the newest band), `$orderby=sentDateTime desc`, the summary `$select`, `$top` 1,000. A round over the band a coverage lacks lists that band alone.
- **Graph mail is bound to no scope** (`Client::scope_bound` false), so io-pimdir widens by band rounds and keeps the checkpoint.
- **Order within a round.** The band's pages land first, their bodies following them (`bodies-follow-their-page`); then the pass lands as the round's last page with the link, so nothing changed while the band was listed is lost: the pass lists every member in scope, by its binding, by the summary the band read, else by one read in JSON batches, and states gone every member the store binds or the band listed that it no longer lists. The pass runs within one request: resumed, it starts again.
- **Deltas.** A change outside the scope is dropped by its `sentDateTime`; a bound member is listed by its flags; a member in scope the store does not bind is named by its summary, the row's own on a whole-folder link, else read in JSON batches of 20 (`GET /messages/{id}?$select=…`), a 404 left out and any other unanswered summary failing the listing.
- **How the two links relate.** Both are links over the whole folder; they differ in what their rows carry. Whatever scope a later run asks for, the stored link serves it: a narrower scope follows it, a wider one lists its band and keeps it. Only an expired link (410), or `--full`, opens a round, which makes a link of the kind the scope it lists calls for. The checkpoint is tagged (`summary <link>`, `ids <link>`); a bare link, stored before, is a whole-folder link unless the coverage is bounded, when a `$filter` made it, and it is refused, which opens a round.
- **Messages with no date.** A Graph band listing on `sentDateTime` never lists them, yet they lie in every band: the last page of a band round lists those the store binds by their last-synced flags (`Held::undated`), the link reporting what changed on them since.

Graph contacts and events are unchanged.

## Not done

- Not run against Microsoft 365 in this change: the live test `a_graph_scope_widens_by_its_band` compiles and stays ignored.
- The first round under a scope passes over the whole folder once, ids only (about 1,000 rows a request, ~100 bytes each); a folder of 100,000 mails costs about 200 requests before the round closes, after its band has landed.
