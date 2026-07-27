---
cairn: change
id: qresync-enumeration
status: landed
created: 2026-08-01
---

# Incremental IMAP enumeration (QRESYNC/CONDSTORE)

## Why

`enumerate` did a full `FETCH 1:* (UID FLAGS ENVELOPE RFC822.SIZE)` of every
mailbox on **every** sync (and every settle pass), ignoring the stored cursor —
so a no-change sync against a real account (Fastmail) re-scanned every envelope,
several times over. The server already offers the delta (it reports
`UIDVALIDITY`/`HIGHESTMODSEQ` on SELECT and advertises QRESYNC/CONDSTORE); neverest
just threw it away (`checkpoint: ReplicaCheckpoint(Vec::new())`, `_cursor` ignored).

io-replica already supports delta enumeration (`ReplicaRemoteSnapshot.complete =
false` + `vanished` → `delta_candidates`), and io-imap already exposes QRESYNC
(`select_qresync`, `ImapMailboxSelectData.{highest_mod_seq,uid_validity,
vanished_earlier,changed}`, `enable`). So this is a pure neverest wiring change.

## What

- **ENABLE QRESYNC/CONDSTORE on connect** when the server advertises QRESYNC
  (`ImapClient` now keeps the capabilities). Adds `supports_qresync` and a
  `select_delta` (QRESYNC SELECT) helper.
- A new lean `ImapClient::enumerate(mailbox, cursor)`: with a matching cursor and
  QRESYNC it does `SELECT (QRESYNC (uidvalidity highestmodseq))` — the server
  streams only the messages changed since the modseq and the UIDs that vanished,
  and **nothing is fetched when nothing changed**. Otherwise a full
  `FETCH 1:* (UID FLAGS)` snapshot. No ENVELOPE is fetched at all — the link id
  is resolved later at the `Meta` tier, so enumeration only ever needs UID+FLAGS.
- `EmailRemote::enumerate` encodes/decodes the cursor as `(UIDVALIDITY,
  HIGHESTMODSEQ)` in the `ReplicaCheckpoint`, and returns a delta
  (`complete=false` + `vanished`) or a full snapshot accordingly. A UIDVALIDITY
  change falls back to a full snapshot; a malformed/absent checkpoint too.

Verified against two live Stalwart servers: a no-change second sync issues a
QRESYNC SELECT per mailbox and **zero FETCH** commands; appending a new message is
still picked up by the delta (`1 pulled`, body hydrated). Unit test for the cursor
codec; relay integration unregressed.

## Scope / non-goals

- QRESYNC-first: a CONDSTORE-only server (no QRESYNC) still full-enumerates; the
  `FETCH … CHANGEDSINCE` fallback is a later refinement.
- No io-imap or io-replica change (both already had the surface).
