---
cairn: change
change: targeted-meta-fetch
---

## ADDED Requirements

### Requirement: Meta and size fetches are targeted
The `Meta` fetch (link id + summary) and the largest-first size probe SHALL
fetch **only the handles being processed** (a `UID FETCH <handle-set>`), never
the whole mailbox: the `Meta` fetch as `(UID FLAGS ENVELOPE RFC822.SIZE)`, the
size probe as size-only `(UID RFC822.SIZE)` (no ENVELOPE). So an incremental
sync's silent pre-download work scales with the number of changed messages, not
the mailbox size — no whole-mailbox ENVELOPE sweep runs to resolve a handful of
link ids or to order a download. This mirrors the lean, targeted `enumerate`
(QRESYNC delta); a first sync stays inherently heavy (every new message's link
id is fetched once), but the redundant second sweep is gone.
