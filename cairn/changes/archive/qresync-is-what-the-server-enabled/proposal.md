---
cairn: change
id: qresync-is-what-the-server-enabled
status: landed
created: 2026-11-15
---

# QRESYNC is what the server enabled

## Why

Live testing of 2026-11-15 (ledger line N20 of pimalaya/.github cairn/changes/cli-alignment-2026-11-15, HIGH): on iCloud, neverest never received new mail in a mailbox it had synced. src/imap/client.rs sent `ENABLE CONDSTORE QRESYNC` and dropped the list `enable` returns; `supports_qresync` read the advertised capability alone. iCloud advertises QRESYNC, answers the `ENABLE` with a bare `OK` and no `* ENABLED`, and its QRESYNC `SELECT` then returns no `FETCH`: the delta read no change, took the new `HIGHESTMODSEQ` as its checkpoint and stepped over every arrival, flag change and expunge for good. The heads (src/client.rs `heads`) were chosen from the same gate.

## What

- `ImapClient` keeps whether the server's `ENABLED` named QRESYNC (`qresync`), and `supports_qresync` answers that: the delta, the delta head and the source's heads stand on it.
- A server advertising QRESYNC and enabling nothing is read as one without: a delta answers a round, page by page, the band heads alone are stated.
- The fake IMAP server can answer `ENABLE` with a bare `OK` (`enable_nothing`).

## Decisions

- **No CONDSTORE delta**: neverest has no `CHANGEDSINCE` path; a server without QRESYNC enabled is read by rounds, as one advertising none was. A CONDSTORE delta could not state removals anyway.
- **io-imap untouched**: the list was there to read; its watch is the live tester's agent's.
