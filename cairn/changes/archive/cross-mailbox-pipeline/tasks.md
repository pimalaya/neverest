---
cairn: tasks
change: cross-mailbox-pipeline
---

- [x] `SyncWriteStore` wrapper: `ReplicaStorage` delegating reads, serialising
      `write` on a shared `Mutex<()>`; exposes `inner()` for read helpers.
- [x] `CachedFetchRemote` wrapper: `ReplicaRemote` serving `fetch(Full)` from a
      `(collection, handle) → ReplicaFetchedItem` cache, falling back to a real
      `EmailRemote` on miss; enumerate/push unreachable.
- [x] Extract the spine: a `MailboxTargets { mailbox, targets: Vec<(handle, size)> }`
      producer (enumerate + meta + itemize + push), reusing the current logic minus
      hydrate.
- [x] Phase 1: pre-create collections serially; then a worker pool (own store
      handle + own connection each) over a mailbox queue, collecting targets and
      report patches (merged after).
- [x] Phase 2: global largest-first per-mailbox batches on one queue, work-stolen;
      stream bodies to blobs; build the fetched-item cache; one global progress bar.
- [x] Phase 3: serial per-mailbox `Full` upgrade via `CachedFetchRemote`.
- [x] Progress: `Scanning` / `Downloading n%` / `Writing` phase bars.
- [x] Build, fmt, clippy clean; unit tests (cache remote hit/miss; write lock).
- [x] Live Stalwart, many mailboxes: bodies present & correctly linked; idempotent
      re-sync (no re-fetch, no ghosts); connections saturated across boundaries;
      concurrent writes safe (no Busy/corruption).
- [x] Fold delta into `cairn/spec/sync.md`; write log entry.
