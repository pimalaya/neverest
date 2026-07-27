---
cairn: change
change: largest-first-restore
---

## MODIFIED Requirements

### Requirement: Hydration may run concurrently, largest-first
Full-tier hydration SHALL fetch bodies in **batches** — one `UID FETCH <set>
(UID BODY.PEEK[])` streaming K bodies (`BATCH_SIZE`, default 64) in a single
response — so N bodies cost ~N/K round trips per connection rather than one round
trip per message. Each message is routed to its own streaming sink by the **UID
on its own FETCH line**, so an out-of-order server response still lands
correctly; a body line without a parseable UID SHALL fail the batch so the caller
falls back to per-message fetches rather than misroute. Handles SHALL be ordered
**largest-first** using each item's body size read from the store meta (the `v:1`
summary's `size`, already local — no size probe), so the heavy messages are
front-loaded and the progress counter accelerates to a smooth finish instead of
freezing on a big message that landed last; when sizes are unavailable (e.g. the
cross-side copy path) it falls back to UID order. Batches SHALL be chunked from
the ordered handles and work-stolen across the persistent connection pool (a
worker with heavy batches naturally takes fewer). On any batch error the fetch
SHALL fall back to per-message fetches; content-addressing makes the partial retry
idempotent. The pool is **persistent**: one primary connection is opened up front
for the sequential verbs, more are opened lazily up to a budget on the first
`Full` batch and kept for the run, so their auth is paid once, not per batch. The
budget defaults to 4, is configurable per account (`connections`) and overridable
by a `sync --connections` flag, and SHALL stay under the backend's per-account
connection cap; a trivial batch falls back to the primary connection. Body bytes
stream lock-free into the blob store; the engine serialises the index write on
the single-writer store afterwards.
