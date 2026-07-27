---
cairn: change
id: batched-body-fetch
status: landed
created: 2026-08-01
---

# Batched body fetch: one FETCH per batch, not per message

## Why

After the incremental-refcount and SELECT-once fixes, a first sync is bound
almost entirely by **body-fetch round trips**: neverest issued one
`UID FETCH <uid> BODY.PEEK[]` per message, blocking on each. With W connections
that is ~N/W sequential round trips. On a high-latency server (Fastmail ~75 ms
RTT) that dominates; raising the connection count past the server's per-account
cap made it *slower* (throttling), confirming that more connections is not the
lever. mbsync stays fast on one connection by pipelining — keeping many fetches
in flight so it pays the round trip roughly once.

## What

Fetch bodies in **batches**: one `UID FETCH <set> (UID BODY.PEEK[])` streams K
bodies back in a single response, so N bodies cost ~N/K round trips per
connection instead of N.

- **io-imap** gains an additive `ImapMessageFetchStreamBatch` coroutine and a
  `fetch_bodies_stream` client method: it parses each `* n FETCH (UID u BODY[]
  {len}` item, announces the UID (`MessageStart`), streams that body to a
  per-message sink, then `MessageEnd`, looping to the tagged status. Each body is
  routed by the **UID on its own FETCH line**, so out-of-order server responses
  still land correctly; a body line with no parseable UID errs (`UidMissing`) so
  the caller can fall back rather than misroute. Existing single-message fetch is
  untouched.
- **neverest** hydrates the `Full` tier in batches of `BATCH_SIZE` (64),
  work-stolen across the connection pool: each worker drains a batch and issues
  one batched FETCH on its connection. Each message opens its own streaming
  `HydrateSink` (blob writer + hasher + header capture), so bodies never buffer
  whole and stay content-addressed. On any batch error it falls back to
  per-message fetches (idempotent, since blobs are content-addressed).
- The largest-first **size probe is removed**: work-stealing balances load
  without it, and it was a redundant round trip. Handles are sorted by UID so
  consecutive ids collapse to ranges in the command.

Verified live (Stalwart): 200 messages fetched in **4 batched FETCHes** (was
200), every unique body marker present exactly once, no blob mixing two bodies, a
clean idempotent re-sync — routing is correct end to end. ~5–6× faster body
fetch even on localhost (no latency to save); far more on a high-latency link.
