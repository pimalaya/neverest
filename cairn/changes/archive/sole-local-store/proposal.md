---
cairn: change
id: sole-local-store
status: landed
created: 2026-08-01
---

# pimdir is the sole local store

## Why

Action plan M7: pin the invariant that makes the store trustworthy as the one
local copy under every client. It is already true by construction, but unstated,
so it is easy to erode. Two points to make explicit:

1. **Single local copy.** A message body lives once, content-addressed in the
   pimdir blob store (deduped across sides and mailboxes). Neverest keeps no
   parallel local copy in another format.
2. **m2dir is a remote *source*, not a second local store.** When a side is a
   maildir/m2dir, it is one of the two *sides being synced* (interop with an
   existing on-disk store), reconciled through the same pimdir pivot as an IMAP
   side — not a local cache alongside pimdir. The file-per-item local store is
   superseded by the indexed pimdir store (its portable export profile is pimdir
   `EXPORT.md`).

## What

Spec-only clarification (the behaviour already holds): a `sync` requirement stating
the pimdir store is the sole local copy and that a maildir/m2dir side is a synced
source, plus the store-path convention (`pimdir.db` + `objects/` per account).

## Scope / non-goals

- No code change; this pins an existing invariant so future work does not add a
  second local copy.
- Body-retention transient/relay modes (never keeping the local body) remain future
  design (LOCAL_STORE_PLAN §5).
