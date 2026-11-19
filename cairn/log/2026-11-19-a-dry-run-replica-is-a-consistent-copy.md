---
cairn: log
change: a-dry-run-replica-is-a-consistent-copy
date: 2026-11-19
---

# A dry run's replica is a consistent copy

pimdir audit line N1 (pimdir cairn/changes/audit-2026-11-12): `--dry-run` cloned the store file by file, pimdir.db and its WAL included, while producers write the store under no lock neverest holds, the file copy of a live store pimdir STORAGE §8 forbids. io-pimdir 0.17 offers `PimdirReader::backup`, which copies every body rather than linking it, so a dry run over a mail store would read and write gigabytes; neverest takes the copy itself instead, by §8's rules.

## What landed

- src/sync/driver/upkeep.rs: `DryRunReplica::new` takes objects.lock shared (`File::lock_shared`, `flock` or `LockFileEx` as §8 says) before anything is read, waiting out a collector, then copies pimdir.db through `VACUUM INTO` on a read-only connection (`snapshot_database`), then links the bodies and copies the rest; `clone_dir` leaves the database files and the root's lock files out.
- src/sync/driver/tests.rs: `a_dry_run_replica_waits_out_a_collector`, failing on the code before (the replica was taken under a held exclusive lock). `stub_store` builds a real store, the snapshot needing a database; the byte comparisons became an inode check and the replica opening.

## Remove contract tests (audit Y2)

- src/imap/fake.rs: `FakeImap::without_uidplus` drops UIDPLUS from the greeting; a kept append then names no `APPENDUID`, and a kept message is named by the `Message-ID` its literal states.
- src/sync/driver/tests.rs: `a_remove_into_a_target_holding_the_identity_without_uidplus_is_held` (no relocation, nothing marked `\Deleted`) and `an_append_to_a_server_without_uidplus_is_found_by_its_message_id` (accepted with the UID the search finds). Both rules landed in 52f83a9; these pass on the code before, being contract tests and not a fix.

## Spec

- [sync](../spec/sync.md): *A dry run works on a replica that shares the bodies* names the snapshot, the shared staging lock and the lock files, scenario "A dry run while a collector runs".
