---
cairn: change
change: qresync-enumeration
---

# Delta

## ADDED Requirements

### Requirement: IMAP enumeration is incremental (QRESYNC)
Enumeration SHALL carry a per-mailbox cursor `(UIDVALIDITY, HIGHESTMODSEQ)` in the
`ReplicaCheckpoint`. On a QRESYNC-capable server (ENABLEd on connect) with a cursor
whose UIDVALIDITY still matches, `enumerate` SHALL issue a QRESYNC
`SELECT (QRESYNC (uidvalidity highestmodseq))` and return a **delta**
(`complete = false`): only the messages changed since the modseq, plus the vanished
UIDs — issuing **no FETCH when nothing changed**. Without a usable cursor (first
sync, UIDVALIDITY change, malformed checkpoint) or on a non-QRESYNC server it SHALL
return a **full** `FETCH 1:* (UID FLAGS)` snapshot (`complete = true`). Enumeration
SHALL fetch UID and FLAGS only — never ENVELOPE — since the link id is resolved at
the `Meta` tier.

## MODIFIED Requirements

## REMOVED Requirements
