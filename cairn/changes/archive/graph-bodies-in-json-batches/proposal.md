---
cairn: change
id: graph-bodies-in-json-batches
status: landed
created: 2026-10-05
---

# Graph bodies arrive in JSON batches

## Why

A Graph mail source fetched each body with its own `GET /messages/{id}/$value`, one after the other on the single connection an HTTP source keeps. IMAP fetches 64 bodies per `UID FETCH` on up to four connections. Reported by MOA: the first sync of a 1.2 GB Microsoft 365 box was still running after ten minutes, where a 2 GB IMAP box (Posteo) took one. At one round trip per message, tens of thousands of messages cost hours.

The code said "Graph having no batched body fetch". That is wrong: checked on the Pimalaya test tenant, a JSON batch (`POST /$batch`) of `$value` gets answers each body as a base64 string (`Content-Type: text/plain`), byte-identical to the direct get. Twenty bodies took ~0.35 s in one batch against ~1.2 s one by one from here, and the gap grows with the latency to Graph.

Graph also throttles (429, `Retry-After`), and neverest retried nothing: a throttled get failed its batch.

## What (design)

- `fetch_messages` sends the ids in JSON batches of 20 (`MSGRAPH_BATCH_MAX_REQUESTS`, io-msgraph's existing `batch`), each request id the index of its message, matched back whatever order Graph answers in, and commits the decoded bodies in request order.
- A sub-response 429 or common 5xx is sent again in the next batch after the longest `Retry-After` stated (else 1, 2, 4, 8 s), capped at 120 s, at most four times; a batch throttled as a whole likewise.
- Any other refusal, or a body that is not base64 text, is left out: the hydration's existing guard fetches every handle a batch did not answer one by one, which surfaces the real error as before.
- `get_message_stream` (one targeted body) keeps the direct get.
- New dependency: `base64` 0.23, already in the lock through io-msgraph, on the `msgraph` feature.

Not in scope: several connections for HTTP sources (one stays the budget), or fetching very large messages outside the batch (a batch holds its twenty bodies in memory, base64 included).
