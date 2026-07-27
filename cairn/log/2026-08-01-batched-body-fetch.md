---
cairn: log
change: batched-body-fetch
landed: 2026-08-01
---

# Batched body fetch: one FETCH per batch, not per message

With the CPU (O(N²)) and SELECT-once fixes in, a first sync was bound almost
entirely by body-fetch round trips — one `UID FETCH <uid> BODY.PEEK[]` per
message. Raising the connection count past Fastmail's per-account cap made it
*slower* (throttling), confirming batching (mbsync's lever) as the fix.

Bodies now fetch in batches of `BATCH_SIZE` (64): one `UID FETCH <set> (UID
BODY.PEEK[])` streams 64 bodies in a single response, so N bodies cost ~N/64 round
trips per connection.

- **io-imap** (additive, existing consumers untouched): new
  `ImapMessageFetchStreamBatch` coroutine — parses each `* n FETCH (UID u BODY[]
  {len}` item, emits `MessageStart{uid}`, streams that body
  (`BodyChunk`/`WantsStream`), then `MessageEnd`, looping to the tagged status.
  Routing is by the **UID on each FETCH line** (out-of-order responses land
  correctly; `routes_by_uid_not_by_position` test). A body line without a
  parseable UID errs `UidMissing` so the caller can fall back rather than
  misroute. New `ImapClientStd::fetch_bodies_stream` driver with per-message
  open/done sinks and the 128 KB body buffer. 8 new unit tests (207 total green).
- **neverest**: `Client::fetch_bodies` + backend `fetch_bodies` (parse uids,
  select-cached, inner batched stream). `hydrate_batch` opens a streaming
  `HydrateSink` per message, ticks progress per message, and on any batch error
  falls back to per-message fetches (idempotent — blobs are content-addressed).
  `fetch_full` chunks handles into batches, work-stolen across the pool. The
  largest-first **size probe is removed** (redundant round trip; work-stealing
  balances load); handles are UID-sorted so consecutive ids collapse to ranges.

Verified live (Stalwart): 200 messages fetched in **4 batched FETCHes** (was 200)
across 4 connections; all 200 unique body markers present exactly once, no blob
holding two bodies, and a clean idempotent re-sync — the UID→body→link chain is
correct end to end. ~5–6× faster body fetch even on localhost (0.9s vs ~5s for
2000 messages), and far more on a high-latency link where round trips actually
cost. fmt/clippy clean.

Spec updated: `sync` (MODIFIED: "Hydration may run concurrently, largest-first" →
batched fetch, work-stealing, no size probe; "Bodies transfer with bounded
memory" → per-message streaming sink + 128 KB copy buffer).
