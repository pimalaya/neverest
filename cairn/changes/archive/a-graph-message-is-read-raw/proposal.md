---
cairn: change
id: a-graph-message-is-read-raw
status: landed
created: 2026-11-13
---

# A Graph message is read raw

## Why

pimdir audit line D2, its Graph half (pimdir cairn/changes/audit-2026-11-12/tasks.md). neverest 3451138 routed IMAP and Gmail through io-pimdir's `derive_meta` and left Graph naming a message from `subject`, `sentDateTime` and `from`, values Graph decodes by rules of its own: io-msgraph 0.7 modelled no `internetMessageHeaders`, the raw read pimdir STORAGE Annex A.1 names for Graph. A Graph member whose decoded summary differs from the format's reading is minted apart from its own message where another reading wrote its row, and a key is never derived again.

io-msgraph 92c28d5 carries the member (`MsgraphMessage::internet_message_headers`, name and value pairs in Graph's order, repeated names kept), and a probe of the test tenant on 2026-11-13 showed a listing and a delta page return it under `$select` for the inbox and for Sent Items alike (io-msgraph cairn/log/2026-11-13-message-internet-headers.md). Annex A.1, as corrected the same day, reads a source listing every occurrence of a field from the first of that name, and names a member Graph answers no header field for from `subject`, `sentDateTime` and `from`.

## What

- io-msgraph is pinned by git at 92c28d5 in `[patch.crates-io]`, the dependency line staying `0.7`.
- Every read naming a Graph mail member selects `internetMessageHeaders`: the whole-folder delta, a band page, a listing by arrival, the summary batch and the `Meta` fetch. The pass closing a round under a scope names none and keeps its ids, dates and flags.
- A row listing header fields is named off them through `derive_meta`, the fields a summary reads laid out as a header block, each from its first entry, as Gmail's path does. The size, the flags and the attachment mark stay Graph's own members.
- A row listing none keeps the reading of 3451138, from `internetMessageId`, `subject`, `sentDateTime` and `from`.
- The `Meta` fetch moves to io-msgraph's typed get, which takes a select and an expand since 92c28d5.
- A delta keeps of a row it names what its page lists of it, never the row: a row now carries every header field of its message.

## Not in this change

- What a draft answers for `internetMessageHeaders`: not probed, Drafts held no message.
- A delta link stored before this change: its `$select` is the link's own, so its rows list no header field until a round makes a new link.
- The page sizes: unchanged, and not measured against Graph with the larger rows.
