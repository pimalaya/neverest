---
cairn: tasks
change: dry-run-replica-shares-bodies
---

# Tasks

- [x] Replace `dry_run_replica` with a `DryRunReplica` guard built beside the
      real store.
- [x] Hardlink the blob tree, copy everything else, fall back to a copy where a
      link is refused.
- [x] Remove the replica on drop, and clear what an earlier run left behind.
- [x] Log the preparation at `debug`, and an unshareable blob tree at `info`.
- [x] Drop the `if dry_run { remove_dir_all }` line on the way out of `run`.
- [x] Cover it: bodies shared, index copied, a write to the replica leaving the
      store alone, removal on drop, and a leftover cleared.
- [x] CHANGELOG.md.
- [x] Fold the delta into cairn/spec/sync.md and log it.
