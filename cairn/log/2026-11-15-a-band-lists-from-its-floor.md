---
cairn: log
change: a-band-lists-from-its-floor
date: 2026-11-15
---

# A band lists from its floor

Ledger line N17 of the CLI alignment round of 2026-11-15, from live testing: a widening deleted from the store mail Gmail and Graph still held (13 messages on Gmail INBOX and SENT, 24 on Graph Sent Items), dated in the band by their `Date` and received or submitted after it.

## What landed

- src/client.rs: `floor_of`, the scope a connector lists under when its date filter reads another date than the `Date`: the floor alone, never a band's ceiling.
- src/gmail/client.rs: a round lists by `floor_of`; `scope_query` writes `after:` alone, never `before:`. The scripted wire takes a responder answering by the request line.
- src/msgraph/client/mail.rs: `enumerate` lists heads, deltas, band pages and the pass by `floor_of`; a band page still resumes below the second the page before ended at. The fake folder's rows may state a `Date` header apart from `sentDateTime`.
- Tests: `a_band_round_keeps_a_message_received_after_its_date` (src/offline/driver.rs) and `a_band_round_keeps_a_message_sent_after_its_date` (src/msgraph/client/mail.rs), each dropping the message on the code before. `widenings_list_their_band_and_keep_the_link`, `a_whole_folder_link_serves_every_scope_after_it` and `a_band_round_resumed_after_a_delete_misses_nothing` count the coverage listed again.

## Left

- A member dated after its arrival or submission by more than the margin (Gmail two days, Graph none) and before the round opened is still missed by the floor and found absent, in a band as in the main scoped round: the engine's rule (io-pimdir list_unstamped_bindings.sql, `s.date < r.round_started_at`) holds a narrowed listing names every member dated before it opened. Rare (a sender's clock running ahead); a library question.
- Requirement *A Graph mail is dated by its `Date`* still calls `sentDateTime` Graph's reading of the `Date` header; a row listing its header fields is dated by them.

## Capabilities moved

- [sync](../spec/sync.md): *A mail sync lists within a scope on the `Date`* and *A provider narrows a scope with a margin* amended.
