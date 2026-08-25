---
cairn: tasks
change: duplicate-link-id-freeze
---

# Tasks

- [ ] Bump io-replica and io-pimdir to the releases carrying the freeze (same
      change id in both repos); nothing here works before they land.
- [ ] Read the ambiguity off the projection (`ReplicaStatus::Ambiguous` plus the
      binding's handles) in `src/offline/driver.rs`, beside the conflict pass
      (`itemize_*`), and carry it into the report.
- [ ] `SyncReport` gains a warnings section (`src/sync/report.rs`), rendered in
      text and `--json`, naming the collection and every handle; add its
      `*Output` entry and the json_schema.rs registration.
- [ ] Re-report it on every run, as `conflicts` already are, and word it as an
      ambiguity neverest will not resolve, never as an invalid mailbox.
- [ ] Fix the silent write: an append performed by the sync appears as a hunk,
      and `already in sync` means nothing was written (seen in step 3 of the
      proposal, where a resurrected message was appended with an empty report).
- [ ] Unit tests: the warning renders in both formats with its coordinates; a
      run that appends reports a hunk.
- [ ] `tests/duplicates.rs`, ignored by default like the other live tests,
      against `tests/stalwart2.sh`: seed one copy on A and two on B, sync,
      assert no hunk for that identity and a warning naming both UIDs; delete
      the bound copy on B, sync, assert A's copy survives and no delete is
      pushed; drop the right side's checkpoint, sync, assert nothing is appended
      to A.
- [ ] `cargo test --all-features`, `cargo clippy --all-features --all-targets`,
      `cargo fmt`.
- [ ] Fold `delta.md` into `cairn/spec/sync.md`; add the `cairn/log` entry; mark
      the change `landed` and archive it.
