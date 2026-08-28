---
cairn: change
id: dry-run-replica-shares-bodies
status: landed
created: 2026-08-28
---

# A dry run shares the bodies it is not going to change

## Why

A dry run works on a throwaway replica of the store, so no checkpoint advances
and nothing reaches a server. That replica was a deep copy of the whole store
into the temporary directory, taken before the first spinner and logged at no
level.

It stopped being cheap as soon as stores stopped being small. A mail account's
store is its blob tree: 2.5 GB over 9511 files for one posteo account, of which
13 MB is the index and the rest is bodies. Every dry run read and wrote all of
it, several silent seconds before anything appeared on screen, and on a machine
whose `/tmp` is a tmpfs it spent those gigabytes of memory rather than disk.

The copy is also almost entirely pointless. Bodies are content-addressed, which
is to say immutable: nothing rewrites one in place, and a dry run does not purge,
so the replica needs the same bytes rather than its own. What it genuinely needs
of its own is what it writes to, and that is the SQLite database.

Separately, the replica outlived the runs that failed. Removing it was a line on
the way out of `run`, so any `?` before it, a credential that would not resolve,
a refused mode change, a store that would not open, returned early and left the
copy behind.

## What

- The replica is built beside the real store instead of under the temporary
  directory, so the two share a filesystem and the bodies can be hardlinked.
- Only the blob tree is shared. Everything else is copied, so a dry run's writes
  cannot reach the real store. A file the rule misjudges is copied, never shared:
  the cost of being wrong is a slower dry run, not a corrupted store.
- The replica is a guard whose destructor removes it, so it goes however the run
  ends, and a run clears what an earlier one left behind, a release build
  aborting on panic without running destructors.
- The preparation is logged at `debug` with its elapsed time, and a blob tree
  that could not be shared says so at `info`, that being the slow case this
  exists to avoid.

## Not in scope

**No reflinks.** ext4 has none, and a hardlink already costs a directory entry
and nothing else. A filesystem offering copy-on-write would not make the shared
tree any cheaper than shared.

**No sharing of the index.** It is small, and it is the one file a dry run
writes to.
