---
cairn: change
change: select-once-per-connection
---

## ADDED Requirements

### Requirement: A connection SELECTs a mailbox once per run of commands
An IMAP connection SHALL cache the mailbox it currently has `SELECT`ed and skip a
redundant `SELECT` when the next command targets the same mailbox, so a run of
commands on one mailbox — most importantly a batch of body fetches — pays a single
`SELECT`, not one per command. Every select path SHALL record the selection so a
cached skip is always correct. For a hydrate of N bodies across a W-connection
pool this makes `SELECT`s ~W rather than ~N, halving the fetch path's round trips
over a high-latency link without changing what is fetched.
