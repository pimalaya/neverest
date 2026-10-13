---
cairn: change
id: a-graph-round-lists-its-head
status: landed
created: 2026-10-13
---

# A Graph mail round lists its head

## Why

A first sync of a large Microsoft 365 folder keeps its round open for many runs (MOA stops a run after a page with `--prime`, and a long delta round spans several), and its later pages list older mail: a message arriving meanwhile showed only at the first delta after the round closed. pimdir's `round-head` change (SYNC §4, §5, vectors 62 to 69, vectors 65 and 66 being Graph's case) lets a run resuming an open round list the round's head first, and io-pimdir `07bd316` asks for it. Graph gives its delta link with a round's last page only, so it has no checkpoint to list a delta head from; it lists a band head, by `receivedDateTime`.

## What (design)

- **Graph mail lists band heads** (`Client::heads`); Graph contacts and calendars, listed whole, none. A delta head is answered none.
- **The head** is the folder's messages Graph received from two days before the instant the round opened (`round_started_at`, read as RFC 3339, SQLite's `strftime` form or its plain `YYYY-MM-DD HH:MM:SS`), through `/messages` with `$filter=receivedDateTime ge T` and `$orderby=receivedDateTime desc` (Graph orders on a property only when the filter names it first), the summary `$select`, the MAPI size `$expand` (refused once with a 400, the session asks without it from then on, as for the delta), `$top` 512, every next link followed, each page read down as it comes. Each message is checked against the run's scope by `sentDateTime`; a member the store binds is listed by its flags, any other by its summary. No checkpoint, no removal. An instant that reads as none lists no head.
- **The last page's obligation** (SYNC §4: a connector listing band heads lists again, in the last page of every round, every member a band head of that round would list then, read no earlier than the checkpoint that page carries). The session keeps, per folder, the instant the run's head listed from. A whole-folder round's last delta page, once the delta link is in, and a band round's last band page list the same listing again and add every member in the round's scope the page does not already list or state removed. Under a scope, the pass is the last page: it lists every member in scope still in the folder, and a member the head named this run (not bound when the run began, not listed by a band page) is read by its summary as any member the band did not list, so nothing is skipped. The engine lists the head first in every run resuming a round, so a run reaching the last page of a round it resumed holds the instant; a run that opened the round listed no head and owes nothing. A round opened afresh, the pass and the last page drop it.
- **The driver** hands the sync verb the source's heads (master `d485913`, shared with Gmail).

## Not done

- Not run against Microsoft 365: no live tenant here. The `receivedDateTime` filter and order are Graph's documented pair; the size `$expand` falls back when refused.
- A delta head on a widening band round, from the stored link: Graph's link is the folder's whole delta, and a band head serves; left for later.
- The band listing pages with `$skip`-based next links: a message deleted above a band round's position shifts the pages, so a member can be skipped by the band and found absent at the band round's close (an existing limit of band rounds, not of heads: the pass of a scoped round reads every member in scope).
