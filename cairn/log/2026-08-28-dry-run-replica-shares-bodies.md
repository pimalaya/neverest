---
cairn: log
change: dry-run-replica-shares-bodies
landed: 2026-08-28
---

# The dry-run replica stopped copying the mail

`dry_run_replica` deep-copied the whole store into the temporary directory
before the run's first spinner, logging at no level. That was cheap when stores
were small and stopped being so: one posteo account's store is 2.5 GB over 9511
files, of which 13 MB is the index and 2.4 GB is bodies, and `/tmp` here is a
tmpfs, so every dry run spent seconds and gigabytes of memory before anything
appeared on screen. It was also the last unlogged multi-second step on the way
into a run, the credential resolution having just been given a spinner of its
own.

**Sharing** (`offline/driver.rs`): `DryRunReplica::new` builds the replica beside
the real store (`<parent>/.<account>-dry-<pid>`) so the two share a filesystem,
and `clone_dir` hardlinks every file under the blob tree while copying everything
else. Bodies are content-addressed, nothing rewrites one in place, and
`sweep_retained` does not run in a dry run, so the replica wants the same bytes
rather than its own. The index is copied, being what the run writes to, and the
rule fails in the safe direction: a file it misjudges is copied, so the cost of
being wrong is a slower dry run rather than a write reaching the real store. A
link the filesystem refuses falls back to a copy.

**Lifetime**: the replica is a guard whose `Drop` removes it, replacing the
`if dry_run { remove_dir_all }` line at the end of `run`, which any earlier `?`
skipped. That is how two 116 KB leftovers from 2026-08-26 came to sit in `/tmp`;
at today's store size each would have pinned 2.5 GB of tmpfs until reboot. Since
a release build aborts on panic and runs no destructor, `new` also clears the
siblings an earlier run left, which is safe because the store lock is held for
the whole run and two runs of one account cannot race for the name.

**Reporting**: the preparation logs at `debug` with its elapsed time and the
linked/copied counts, and a blob tree that could not be shared at all says so at
`info`, that being the slow case the sharing exists to avoid.

Measured on the account that prompted this, as a `cp -al` proxy for the same
syscalls: 0.578 s for the 9511 files, against the seconds and 2.4 GB of tmpfs the
copy took. What is actually copied drops from 2.5 GB to 13 MB.

Verified: 128 unit tests green, four of them new (bodies sharing an inode while
the index does not, a write to the replica leaving the store alone, removal on
drop, a leftover cleared). fmt and clippy clean.

Spec updated: `sync` (ADDED: "A dry run works on a replica that shares the
bodies").
