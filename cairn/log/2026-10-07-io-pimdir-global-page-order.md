---
cairn: log
change: io-pimdir-global-page-order
landed: 2026-10-07
---

# io-pimdir pinned at be03184, pages across collections walk the global order

io-pimdir moves from `ff28408` to `be03184` (pimdir `22f1f2c`). A page of mail across collections walks the new store-wide index `items_by_sort_global` (`sort_key`, `seq`, `collection`), which the hub creates when it opens an older store; `list_mail_page_filtered` and `search_mail` keep their collection test off `items_by_seq`, so the planner walks the index instead of reading and sorting every live row. A reader of a store still lacking the index pages by a scan and a sort. neverest's code is unchanged: it pages no mail across collections, and the index is created by the hub it opens through. Tests run with the default features and with `--no-default-features --features rustls-ring,imap,smtp,dav,msgraph,gpeople,gcal,gmail`, as MOA builds it.

Capabilities moved: none.
