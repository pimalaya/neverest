---
cairn: tasks
change: an-undated-search-may-be-refused
---

# Tasks

- [x] The fake IMAP server refuses every `HEADER Date ""` search on demand (`refuse_undated_search`)
- [x] Regression test `a_round_goes_on_when_the_undated_search_is_refused`, failing on the code before (`IMAP SEARCH failed: NO Unexpected exception`)
- [x] `search_round` splits the dated and the undated searches; a refused undated search warns and sets `ImapClient::undated_refused`, kept across a reconnection
- [x] Fold into cairn/spec/sync.md, log, CHANGELOG
