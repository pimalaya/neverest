---
cairn: log
change: an-undated-search-may-be-refused
date: 2026-11-15
---

# An undated search may be refused

Ledger line N18 of the CLI alignment round of 2026-11-15, from live testing: iCloud refuses `NOT HEADER Date ""`, so every date-narrowed round failed there.

## What landed

- src/imap/backend.rs: `search_round` searches the dated messages and the undated ones apart; a NO or BAD to the undated search warns and is not sent again on the connection.
- src/imap/client.rs: `ImapClient::undated_refused`, kept across `reconnect`.
- src/imap/fake.rs: `refuse_undated_search`, the fake refusing every `HEADER Date ""` search with the completion given.
- Test `a_round_goes_on_when_the_undated_search_is_refused`, a scoped round and a band over a server refusing the undated search; it failed on the code before with `IMAP SEARCH failed: NO Unexpected exception`.

## Left

- On such a server an undated message the store does not hold is not listed while a scope holds.

## Capabilities moved

- [sync](../spec/sync.md): *A provider narrows a scope with a margin* amended, with the scenario *A server refusing the undated search*.
