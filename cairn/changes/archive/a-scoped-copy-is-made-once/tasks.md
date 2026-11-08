---
cairn: tasks
change: a-scoped-copy-is-made-once
---

# Tasks

- [x] Regression test first, failing on 8574814, against Stalwart: reproduced (the target held the message twice), but Stalwart 0.16.27 matches no `HEADER Message-ID` search, so the test cannot pass there and moved to the fake IMAP: `a_copy_the_target_holds_out_of_scope_is_bound_not_filed_again` in src/offline/driver.rs, the fake (src/imap/fake.rs) honouring `HEADER Message-ID` in `UID SEARCH`; it fails without the search (the append refused, the copy left provisional)
- [x] The same test with a staged move instead of a copy: no gap on IMAP, its source half moving server side (on Graph the target half may copy, covered by the search; live `a_graph_move_is_delivered_once` green); kept as `a_scoped_move_whose_answer_was_lost_is_made_once` in tests/scope.rs
- [x] IMAP: `ImapClient::find_message` searches `HEADER Message-ID "<id>"`, the brackets against substring hits; `Client::find_message` and the copy path in src/offline/remote.rs bind a free hit
- [x] Graph: `GraphClient::find_message` lists the target folder filtered on `internetMessageId`; checked live on the test tenant on 2026-11-08 (a hit answers the message, a miss an empty page); no fake test, the filter string being the only logic
- [x] Search every create carrying an origin, scope or not, instead of only those a scope excludes: the push knows no scope, a garbled `Date` is in every scope for neverest yet below a `SENTSINCE`, and the search costs one command a copy
- [x] CHANGELOG (Fixed)
- [x] Fold delta.md into cairn/spec/sync.md, log, land
