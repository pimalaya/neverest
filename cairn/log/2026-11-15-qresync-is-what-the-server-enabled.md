---
cairn: log
change: qresync-is-what-the-server-enabled
date: 2026-11-15
---

# QRESYNC is what the server enabled

Ledger line N20 of the CLI alignment round of 2026-11-15, from live testing on iCloud: no new mail ever reached a synced mailbox.

## What landed

- src/imap/client.rs: the `ENABLED` list kept; `qresync` true only when it names QRESYNC; `supports_qresync` reads it, so the delta, the delta head and `Client::heads` follow.
- src/imap/fake.rs: `enable_nothing`, an `ENABLE` answered a bare `OK`.
- Test `a_delta_lists_an_arrival_where_qresync_is_not_enabled`: on the advertised-capability gate the delta listed nothing; now it lists the arrival by a round.

## Capabilities moved

- [sync](../spec/sync.md): *IMAP enumeration is incremental (QRESYNC)* and *An open IMAP round lists its head* amended.
