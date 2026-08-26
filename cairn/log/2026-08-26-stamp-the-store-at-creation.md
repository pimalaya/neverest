---
cairn: log
change: stamp-the-store-at-creation
landed: 2026-08-26
---

# The store `init` creates is stamped, so the next run stops refusing it

`StoreState::load` refuses a store directory holding a `pimdir.db` with no
`neverest.json` beside it, reading it as the unnamespaced ancestor whose
collections would every one be looked up under a key nothing was written to. The
premise is that only an older neverest leaves that pair. Nothing ever wrote the
sidecar at creation, so it was every fresh store too: `init` opened the store,
which materializes `pimdir.db`, and stopped there, and the first `sync` loaded
the sidecar before anything else and refused the account it had just
initialized.

The remedy the refusal names did not work either. `reset_replica` recreated the
empty store the same way, sidecar still missing, so `sync --reset` reported
success and the next run raised the same refusal pointing at the reset that had
just run. A new account had no way out of the loop short of reading the source.
`--dry-run` is where it was reported, the copied store in `/tmp` being what the
message names, but the store it copies from was equally refused.

**`StoreState::stamp`** (new) writes a default sidecar: the current layout, and
no recorded derivations, the store it describes having just been emptied.
Creating the database and stamping the layout it was written with are one act,
and the two places that materialize a store now do both: `cli/init.rs` after
`PimdirStore::open`, and `cli/sync.rs`'s `reset_replica` after it recreates the
empty store.

Tests: a stamped store loads back at the current layout instead of being taken
for the ancestor; a stamp forgets what a previous store in the directory
derived; and `reset_replica` leaves a store `StoreState::load` accepts, which
fails without the fix.

Verified: 92 tests green, fmt and clippy clean.

Spec updated: `sync` (MODIFIED: "A store written before namespaced collection
ids is refused" now requires the creator to stamp, with the fresh-account and
reset scenarios).
