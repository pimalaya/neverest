---
cairn: log
change: qresync-enumeration
landed: 2026-08-01
---

# Incremental IMAP enumeration (QRESYNC/CONDSTORE)

Fixed the real-account slowness: `enumerate` used to do a full
`FETCH 1:* (UID FLAGS ENVELOPE RFC822.SIZE)` of every mailbox on every sync (and
every settle pass), ignoring the stored cursor — a no-change Fastmail sync
re-scanned every envelope several times. Now it uses the server-side delta.

`imap/client.rs`: `ImapClient` keeps the server capabilities and ENABLEs
QRESYNC+CONDSTORE on connect when advertised; adds `supports_qresync` and a
`select_delta` (QRESYNC SELECT) helper. `imap/backend.rs`: a lean
`enumerate(mailbox, cursor)` — with a matching cursor and QRESYNC it does
`SELECT (QRESYNC (uidvalidity highestmodseq))`, returning only changed UIDs +
vanished and fetching **nothing** when nothing changed; otherwise a full
`FETCH 1:* (UID FLAGS)`. It fetches UID+FLAGS only (never ENVELOPE — the link id is
resolved at the `Meta` tier). `client.rs`: a backend-neutral `Enumeration` +
`Client::enumerate`. `offline/remote.rs`: `EmailRemote::enumerate` encodes/decodes
the cursor as `(UIDVALIDITY, HIGHESTMODSEQ)` in the checkpoint and returns a delta
(`complete=false`+`vanished`) or a full snapshot; UIDVALIDITY change or a malformed
checkpoint falls back to full.

No io-imap or io-replica change was needed — io-replica already consumes deltas
(`delta_candidates`) and io-imap already exposed QRESYNC.

Verified against two live Stalwart servers: a no-change second sync issues a
QRESYNC `SELECT` per mailbox and **zero FETCH** commands (was: full envelope scan
×mailboxes ×passes); appending a new message is still picked up by the delta
(`1 pulled`, body hydrated into the store). New unit test for the cursor codec;
14 unit tests green, fmt/clippy clean (one pre-existing autoconfig warning), relay
integration unregressed.

Follow-up: a CONDSTORE-only server without QRESYNC still full-enumerates; the
`FETCH … CHANGEDSINCE` fallback is a later refinement.

Spec updated: `sync` (ADDED: IMAP enumeration is incremental via QRESYNC).
