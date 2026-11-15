---
cairn: change
change: dry-run-replica-shares-bodies
---

# Delta

## ADDED Requirements

### Requirement: A dry run works on a replica that shares the bodies
A dry run SHALL work on a throwaway replica of the pimdir store, so that no
checkpoint advances and nothing reaches a server.

The replica SHALL be built beside the real store rather than under the temporary
directory, so the two share a filesystem, and its bodies SHALL be hardlinked
rather than copied. Bodies are content-addressed and therefore immutable, and a
dry run neither rewrites nor purges one, so the replica needs the same bytes and
not its own: a store whose blob tree is gigabytes SHALL cost a dry run some
directory entries, never a read and a write of the whole tree, and never that
much memory on a machine whose temporary directory is a tmpfs.

Everything the run writes to, the index above all, SHALL be copied. A file this
rule misjudges SHALL be copied rather than shared, so being wrong costs a slower
dry run and never a write reaching the real store. A link the filesystem refuses
SHALL fall back to a copy.

The replica SHALL be removed however the run ends, an early return included, and
a run SHALL clear what an earlier one left behind: a release build aborts on a
panic and runs no destructor, so a leftover is a state to meet rather than one to
rule out. Two runs of one account cannot race for it, the store lock being held
for the whole run.

The preparation SHALL be logged with the time it took, and a blob tree that could
not be shared SHALL say so, that being the slow case the sharing exists to avoid.

#### Scenario: A dry run over a mail account's store
- GIVEN an account whose store holds gigabytes of bodies
- WHEN `sync --dry-run` runs
- THEN the bodies are shared rather than copied, and the run starts without
  reading and writing the whole tree

#### Scenario: A dry run that fails leaves nothing behind
- GIVEN a dry run whose credentials fail to resolve
- WHEN it returns
- THEN its replica is gone
