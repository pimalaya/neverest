---
cairn: log
change: a-graph-delta-link-spans-the-folder
date: 2026-10-07
---

# A Graph delta link spans the whole folder

`scoped-mail-sync` bound a Graph mail folder's delta link to its `$filter` on `receivedDateTime`, so each widening of the scope relisted the whole wider scope (about `n²/1000` mails to fill a folder of `n` in steps of 500). The link now spans the whole folder and the scope is listed by date band, the design decided by Clément DOUIN for neverest and the Android bridge alike.

## What landed

- src/msgraph/client/mail.rs (new): the mail listing behind a `MailWire` seam (`GraphClient` serves it, the tests fake it). With no scope a round is the summary delta, as before. Under a scope a round lists `/messages` by `sentDateTime` (`band_filter`: `ge` the floor, `lt` the ceiling, exact, `$orderby=sentDateTime desc`, summary `$select`, `$top` 1,000), cursor `band <next link>`, then `pass`: a fresh delta with `IDS_SELECT` (`id,sentDateTime,isRead,isDraft,flag`) followed to its delta link in one request, listing every member in scope (bare when bound, from the band's cached summary, else read in batches), vanishing what the store binds or the band listed and the pass did not list, checkpoint `ids <link>`. A band round lists its band alone and, on its last page, the bound messages with no date by their last-synced flags. A delta drops what its `sentDateTime` puts out of scope and reads the summaries of unbound members in scope (`metas`: JSON batches of `GET /messages/{id}?$select=…`, a 404 left out, anything else unanswered failing the listing). Links and cursors are tagged (`summary`, `ids`, `band`, `pass`); a bare link is a whole-folder one unless the coverage is bounded, then refused (`CursorRejected`, which opens a round).
- src/msgraph/client.rs: `batch_get` (the retry loop of the body batches, generic over the body read, 404s named `gone`) serving bodies and summaries; the filtered delta (`enumerate_mailbox`, `fresh_delta`, `received_filter`, `delta_page`) removed.
- src/client.rs: `Client::scope_bound` false for Graph mail; `Held` carries `coverage` and `undated`. src/offline/remote.rs: `Bound` reads them from the load; `snapshot` and `keep_in_scope` shared with the tests.
- Tests: seven unit tests in src/msgraph/client/mail.rs, five of them driving io-pimdir's real sync over a fake Graph folder (three widenings list 40 dated messages once each and pass over the folder once, the link kept; a delta drops changes out of scope and reads one summary, the new message's; a round broken in its band resumes it, broken before its pass passes again, a message deleted and one added meanwhile corrected by the pass; a whole-folder link serves a narrower scope, a widening back to no scope lists the band below it, an expired link opens a round under the scope with a new link of ids); `batch_bodies_…` checks a 404 is gone. tests/msgraph.rs: `a_graph_scope_widens_by_its_band` (live, ignored, not run here).

## Capabilities moved

- sync: a Graph mail folder keeps one delta link over the whole folder (added); a provider narrows a scope with a margin, a mail round lands page by page (modified: Graph lists by `sentDateTime` under a scope and is bound to no scope).
