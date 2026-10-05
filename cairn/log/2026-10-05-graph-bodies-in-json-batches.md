---
cairn: log
change: graph-bodies-in-json-batches
date: 2026-10-05
---

# Graph bodies arrive in JSON batches

A Graph mail source fetched one body per round trip, so the first sync of a 1.2 GB Microsoft 365 box ran for well over ten minutes (reported by MOA). Graph's JSON batching answers `$value` gets with base64 bodies, byte-identical to the direct get (checked on the test tenant), so bodies now arrive twenty per request.

## What landed

- `GraphClient::fetch_messages`: JSON batches of 20 raw gets over io-msgraph's `batch`, bodies decoded (standard or URL-safe base64) and committed in request order; 429 and common 5xx retried after the longest `Retry-After` (else 1, 2, 4, 8 s, capped at 120 s), four times at most; anything else left to the per-item fallback.
- Cargo.toml: `base64` 0.23 on the `msgraph` feature (already in the lock through io-msgraph).

## Verification

- Unit: `a_batch_of_raw_gets_names_each_message_by_its_index`, `batch_bodies_decode_out_of_order_and_throttled_ones_are_retried`, `a_batch_body_that_is_not_base64_text_is_left_out`, `a_batch_throttled_as_a_whole_is_sent_again`; 219 unit tests green, clippy clean.
- Live, Pimalaya test tenant: tests/msgraph.rs, 11 green. A whole-mailbox sync stores the same 51 objects as the previous build, through 4 batches and no single get. The mailbox is too small to time the gain (the run is mostly folder listing); twenty bodies took ~0.35 s batched against ~1.2 s one by one with curl.
- Not checked: throttling on a real large mailbox.

## Capabilities moved

- sync: Graph mail bodies are fetched in JSON batches (new).
