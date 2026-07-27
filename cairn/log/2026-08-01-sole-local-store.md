---
cairn: log
change: sole-local-store
landed: 2026-08-01
---

# pimdir is the sole local store

Pinned the invariant that makes pimdir trustworthy as the one local copy under
every client (action plan M7). No code change — the behaviour already holds; this
states it so future work does not add a second local copy.

Two points made explicit in the `sync` spec: (1) a body is held locally exactly
once, content-addressed and deduped in the pimdir blob store, with no parallel
copy in another format; (2) a maildir/m2dir side is a *source being synced*
(interop with an existing on-disk store), reconciled through the same pimdir pivot
as an IMAP side — not a local cache alongside pimdir. The file-per-item local store
is superseded by the indexed pimdir store (portable export via pimdir `EXPORT.md`);
the store lives per account as `pimdir.db` + `objects/`.

Spec updated: `sync` (ADDED: the pimdir store is the sole local copy).
