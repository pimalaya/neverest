---
cairn: log
change: a-relay-fails-one-message-at-a-time
date: 2026-11-17
---

# A relay fails one message at a time

Finding NEV-2 of the CLI family analysis of 2026-11-15.

## What landed

- src/sync/driver/relay.rs: `relay_one` streams through `Exact`, which passes the listed size and holds the last octet until the source ends there (`SizeMismatch` otherwise); recovers each side that failed (`Client::recover`); answers `Result<Result<WrittenItem>>`, the outer error a side that cannot be recovered. `relay_copies` reports a message that failed under `rejected` (action `append`) and goes on.
- Part (a), the hang for good, was already closed by 8c28411 (`std::io::pipe`): `a_relay_the_target_refuses_at_once_returns` guards it and passes on the old logic.
- Fake IMAP: `refuse_next_append_line` (a tagged `NO` at the `APPEND` line) and `keep_appends` (`APPENDUID`).
- Tests: `a_body_longer_than_listed_is_not_appended_cut` and `a_refused_relay_is_reported_and_the_next_message_relayed` fail on the old logic.
- Not run: a live IMAP pair.

## Capabilities moved

- [sync](../spec/sync.md): *A relay appends only where the target's listing is closed* amended.
