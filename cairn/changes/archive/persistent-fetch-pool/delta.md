---
cairn: delta
change: persistent-fetch-pool
---

## ADDED Requirements

(none)

## MODIFIED Requirements

### Requirement: Hydration may run concurrently, largest-first
Full-tier hydration MAY be serviced by a bounded pool of connection-owning
workers running whole-message jobs, scheduled largest-first by the side's own
envelope sizes, so a heavy message overlaps the light ones instead of stalling
the batch. The pool is **persistent**: one primary connection is opened up front
for the sequential verbs, more are opened lazily up to a budget on the first
`Full` batch and kept for the run, so their auth is paid once, not per batch. The
budget defaults to 4, is configurable per account (`connections`) and overridable
by a `sync --connections` flag, and SHALL stay under the backend's per-account
connection cap; a trivial batch falls back to the primary connection. Body bytes
stream lock-free into the blob store; the engine serialises the index write on
the single-writer store afterwards.

## REMOVED Requirements

(none)
