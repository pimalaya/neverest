---
cairn: change
change: batched-body-fetch
---

## MODIFIED Requirements

### Requirement: Hydration may run concurrently, largest-first
Full-tier hydration SHALL fetch bodies in **batches** — one `UID FETCH <set>
(UID BODY.PEEK[])` streaming K bodies (`BATCH_SIZE`, default 64) in a single
response — so N bodies cost ~N/K round trips per connection rather than one round
trip per message. Each message is routed to its own streaming sink by the **UID
on its own FETCH line**, so an out-of-order server response still lands
correctly; a body line without a parseable UID SHALL fail the batch so the caller
falls back to per-message fetches rather than misroute. Batches SHALL be
work-stolen across the persistent connection pool (a worker with heavy batches
naturally takes fewer), and handles SHALL be ordered by UID so consecutive ids
collapse to ranges in the command. On any batch error the fetch SHALL fall back
to per-message fetches; content-addressing makes the partial retry idempotent.
The pool is **persistent**: one primary connection is opened up front for the
sequential verbs, more are opened lazily up to a budget on the first `Full` batch
and kept for the run, so their auth is paid once, not per batch. The budget
defaults to 4, is configurable per account (`connections`) and overridable by a
`sync --connections` flag, and SHALL stay under the backend's per-account
connection cap; a trivial batch falls back to the primary connection. Body bytes
stream lock-free into the blob store; the engine serialises the index write on
the single-writer store afterwards. The obsolete largest-first size probe (a
redundant round trip) is removed — work-stealing balances load without it.

### Requirement: Bodies transfer with bounded memory
A body SHALL be fetched and appended by streaming — fetched straight into the
blob store and appended straight from it — so no full message is held in memory;
peak memory is bounded to a chunk regardless of message size, including in the
batched fetch (each message opens its own streaming sink; the socket-to-blob copy
uses a 128 KB buffer). The `Message-ID` link id and the summary SHALL be read from
the streamed header prefix, so no extra pass over the body is needed. (The local
m2dir backend is not yet chunked; the IMAP path is.)
