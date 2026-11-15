---
cairn: change
id: one-seam-for-the-wire-name
status: landed
created: 2026-08-26
---

# The offline replica sent hub collection ids to the server

## Why

A hub collection is keyed `<namespace>/<name>` and the spec requires the namespace stripped back off "before any call reaches a server, at one seam, so a backend only ever sees the name it gave". `PimRemote::wire_name` is that seam, and two paths went around it.

**Phase 2 of the solo sync.** `run_solo` hydrates bodies through one account-wide work-stealing pool: the workers call `hydrate_batch` on their own connections rather than through `PimRemote`, precisely so a worker finishing one collection's last batch can steal the next collection's. `phase2_hydrate` queued the plan's hub ids and passed them straight down, so every body fetch selected a collection the server has no name for. On IMAP that is not even a legal mailbox name where the delimiter is `.`:

    create collection `imap` on imap: Stream item `2` in `imap/Archives.Charlie` error: IMAP SELECT failed: NO Character not allowed in mailbox name: '/'

The first failing batch stops the pool and fails the namespace, which `run` reports as one errored hunk against the namespace, so a first sync listed thousands of fetches, reported one error, and downloaded nothing.

**A move destination.** `push` strips the source collection and then handed `ReplicaChangeKind::Remove`'s `to` through untouched, so an offline move asked the server to move into `<namespace>/<name>`.

Neither is covered: `Client` is an enum over live backends with no test variant, so nothing below the seam is unit-testable, and the only live tests (`tests/relay.rs`, `tests/submit.rs`) drive `run_pair`. Phase 2 belongs to `run_solo`, which is the offline replica: the shape the wizard writes and the one every ordinary account runs.

## What

- `wire_name` becomes a free function in `offline::remote`, the one seam named once. `PimRemote::wire_name` and the driver's `display_name` both delegate to it, a report naming a collection the way its server does being the same question.
- `phase2_hydrate` strips before the wire call and keeps the hub id as the cache key, which is what Phase 3 looks up through `CachedFetchRemote`.
- `push` strips the move destination.
