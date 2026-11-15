---
cairn: change
id: a-reset-keeps-pending-work
status: landed
created: 2026-10-30
---

# A reset keeps pending work

## Why

Release blocker B4 of the pimdir stable-release review (2026-10-30). `sync --reset` (src/cli/sync.rs:309-335) deletes pimdir.db and objects/ with no check: queued sends, offline creates, unpushed flag changes, parked conflicts and the mail a replica retains after the servers expunged it all go, with nothing on any server to resync them from. It also:

- runs under `--dry-run`, deleting the real store before the dry run;
- under `-m` or `-x` resets the whole store, saying so at `info` only; under `-s`, every source;
- leaves index.db behind, its high-water mark naming a store that no longer exists.

The spec (cairn/spec/sync.md, "A store the format outgrew is refused with its remedy") says a reset "loses nothing but un-pushed local mutation", which understates it.

## What

- `--reset` reads the store first and refuses while it holds work no server has: queued or parked actions, conflicts, placements not `Clean` (staged edits, creates, deletes) and retained items. The refusal lists them, per collection, and names `--force`.
- `--reset --force` never deletes: it moves the store (pimdir.db and its WAL files, objects/, index.db, the sidecar) aside, into a sibling directory named after the account and the instant (`<account>.reset-<UTC instant>`), then starts a fresh store. A store that cannot be read (written by an older draft or a newer neverest) can only be reset this way.
- A plain `--reset` of a store holding nothing pending deletes as before, index.db included.
- `--reset` conflicts with `--dry-run`, `-m`, `-x` and `-s`; `--force` requires `--reset`.
- The stale-store message names `sync --reset --force`.

## Not done

- A rescue export of the pending bodies and queue rows into a readable form: moving the store aside keeps them all, readable by the io-pimdir that wrote them.
