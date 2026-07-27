---
cairn: change
id: select-once-per-connection
status: landed
created: 2026-08-01
---

# SELECT once per connection, not per body fetch

## Why

Studying mbsync (which is markedly faster on a first sync of a large mailbox over
a high-latency server) showed its throughput comes from never paying a round trip
it doesn't have to. neverest was doing the opposite on the hottest path: every
single body fetch issued its own `SELECT` before the `UID FETCH BODY.PEEK[]`, so
downloading N messages cost **2N round trips** — a `SELECT` and a `FETCH` each.
On localhost that is invisible; on Fastmail (~50–100 ms RTT) it doubles an already
round-trip-bound phase. The `Meta` and size fetches re-`SELECT`ed too.

The connection is already sitting on the right mailbox after the first `SELECT`;
re-selecting it before every command is pure waste.

## What

Give `ImapClient` a one-slot cache of the mailbox currently `SELECT`ed on that
connection. A `select_cached(mailbox)` skips the `SELECT` when the connection is
already on that mailbox; every select path (`enumerate`'s plain and QRESYNC
selects, and `select_cached` itself) records the selection, so a cached skip is
always correct — there is no path that changes the selection without recording it.
The per-message fetch, the meta/size fetches, and the flag/move/delete/append-
recovery paths all route through it.

Effect: a run of fetches on one mailbox pays a single `SELECT` per connection
instead of one per command. For a hydrate of N bodies across a W-connection pool,
`SELECT`s drop from ~N to ~W (measured: 6 messages over 4 connections went from a
`SELECT` per body to 5 `SELECT`s total; a 10k mailbox pays ~4 instead of ~10 000).
This halves the fetch path's round trips over a high-latency link. It does not
change what is fetched or how bodies stream; it only removes redundant selects.

(A deeper win — pipelining/batching the body `FETCH`es themselves, mbsync-style —
is a larger, separate change to io-imap and is left as a follow-up.)
