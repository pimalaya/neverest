---
cairn: log
change: persistent-fetch-pool
landed: 2026-07-31
---

# Persistent, configurable fetch pool

Replaced the ephemeral per-batch fetch pool with a persistent one. New `Pool`
(in `client`) holds a primary connection plus lazily-opened extras up to a
budget, kept for the whole run so auth is paid once, not per `Full` batch.
`EmailRemote` now wraps `&mut Pool`: sequential verbs (enumerate, push, `Meta`
fetch, list) run on `pool.primary()`, and a `Full` fetch calls `pool.workers(n)`
and distributes the pool's own `&mut Client` connections into the scoped-thread
workers (`ImapClientStd` is `Send` — `Box<dyn ImapStream>` + buffers, no interior
non-`Send`). `SideCtx` holds the `Pool` in place of a bare `Client`.

The connection budget defaults to 4, is configurable per account via a new
`AccountConfig.connections`, and is overridable by `sync --connections/-j N`
(resolved flag → account → 4, clamped to ≥1); `driver::run` takes it as a
parameter.

Verified end-to-end: the Stalwart roundtrip (five varying-size messages, one
~3 MB) passes through the persistent pool. Unit tests and fmt clean.

Follow-ups: reconnect-on-drop of a dead pooled connection (an errored connection
currently fails its op and reopens next run); per-side rather than per-account
budgets; native chunked m2dir stream.

Spec updated: `sync` (MODIFIED: hydration may run concurrently, largest-first —
now persistent + configurable).
