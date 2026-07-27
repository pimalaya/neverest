---
cairn: log
change: select-once-per-connection
landed: 2026-08-01
---

# SELECT once per connection, not per body fetch

Profiling and an mbsync comparison showed the fetch path was needlessly
round-trip-bound: every body fetch issued its own `SELECT` before the
`UID FETCH BODY.PEEK[]`, so N messages cost 2N round trips. The meta/size fetches
re-selected too. On a high-latency server (Fastmail) this doubles the fetch phase.

`ImapClient` now caches the mailbox `SELECT`ed on its connection (`selected:
Option<String>`, `mark_selected`/`is_selected`). A `select_cached(mailbox)` on the
backend skips the `SELECT` when already on that mailbox; `enumerate`'s plain and
QRESYNC selects record the selection, and the fetch, meta/size, store, move,
delete and append-UID-recovery paths all route through `select_cached`. Because
every select path records the selection, a cached skip is always correct.

Effect: a run of fetches on one mailbox pays one `SELECT` per connection instead
of one per command. Verified live against Stalwart: syncing a 6-message mailbox
over the default 4-connection pool issued **5 `SELECT`s total** (≈one per
connection) rather than one per body, and all six bodies downloaded correctly; a
10k mailbox now pays ~4 selects instead of ~10 000. Bodies stream unchanged.

This is the safe half of closing the wall-clock gap with mbsync. The larger half —
pipelining/batching the body `FETCH`es into few round trips, mbsync-style — is a
separate io-imap change (a multi-message streaming FETCH coroutine) and is left as
a follow-up, since it rearchitects a widely-shared sans-io library and needs its
own careful, well-tested change.

Also present (diagnostic scaffolding, not a spec behaviour): an env-gated
`NEVEREST_PROFILE` breakdown of the coroutine drive loop (storage load/write,
remote enumerate/fetch/push) in `src/offline/prof.rs`, which is what pinned the
O(N²) write and the fetch round-trips. Zero output unless the env var is set.

Spec updated: `sync` (ADDED: A connection SELECTs a mailbox once per run of
commands).
