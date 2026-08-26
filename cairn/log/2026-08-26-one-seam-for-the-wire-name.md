---
cairn: log
change: one-seam-for-the-wire-name
landed: 2026-08-26
---

# The offline replica stopped selecting `<namespace>/<name>` on the server

A hub collection is keyed `<namespace>/<name>` and the namespace comes back off
at `PimRemote::wire_name` before anything reaches a server. Two paths went
around it, and the first one made a fresh solo account download nothing at all:

    create collection `imap` on imap: Stream item `2` in `imap/Archives.Charlie`
    error: IMAP SELECT failed: NO Character not allowed in mailbox name: '/'

**Phase 2** (`driver::phase2_hydrate`). The solo sync hydrates every body across
every collection through one account-wide work-stealing pool, whose workers call
`hydrate_batch` on their own connections rather than through `PimRemote`,
precisely so a worker finishing one collection's last batch steals the next
collection's instead of idling at the boundary. The queue carried the plan's hub
ids and handed them straight to the client, so every body fetch selected a
collection the server has no name for: on a server whose delimiter is `.`, not
even a legal mailbox name. The first failing batch stops the pool and fails the
whole namespace, which `run` reports as a single errored hunk against the
namespace, so the run listed thousands of fetches, reported one error and wrote
nothing. It now strips before the call and keeps the hub id as the cache key,
which is what Phase 3 looks up through `CachedFetchRemote`.

**A move destination** (`remote::push`). The source collection was stripped and
`ReplicaChangeKind::Remove`'s `to` was not, so an offline move asked the server
to move into `<namespace>/<name>`.

**The seam** is now one free function, `remote::wire_name`, named once and
delegated to by `PimRemote::wire_name` and by the driver's `display_name`: what
a report calls a collection and what the wire calls it are the same question,
which the spec already said ("A report SHALL name a collection the way its
server does").

Why nothing caught it: `Client` is an enum over live backends with no test
variant, so nothing below the seam is unit-testable, and both live tests
(`tests/relay.rs`, `tests/submit.rs`) drive `run_pair`. Phase 2 belongs to
`run_solo`, the offline replica, which is the shape the wizard writes and the one
every ordinary account runs. The new test pins `wire_name` itself, the reported
id included; the paths that call it stay uncovered until a fake client exists.

Verified: 93 tests green, fmt and clippy clean, every backend feature subset
compiles.

Spec updated: `sync` (MODIFIED: "A collection is keyed by kind, namespace and
name" now requires every wire call through the seam, the hydration pool and a
move destination named).
