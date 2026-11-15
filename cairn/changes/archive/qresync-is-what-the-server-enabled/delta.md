---
cairn: delta
change: qresync-is-what-the-server-enabled
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: IMAP enumeration is incremental (QRESYNC)
A delta is a QRESYNC `SELECT` only on a server whose `ENABLED` named QRESYNC; one advertising it and enabling nothing answers a round.

### Requirement: An open IMAP round lists its head
Delta heads are stated where the server enabled QRESYNC, not where it advertises it.

## REMOVED Requirements
