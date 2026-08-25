---
cairn: tasks
change: duplicate-link-id-freeze
---

# Tasks

- [x] Bump io-replica and io-pimdir to the releases carrying the freeze (same
      change id in both repos); nothing here works before they land. *(done
      through the addendum below: neither is published, so both are patched to
      their working trees.)*
- [x] Read the ambiguity off the projection (`ReplicaStatus::Ambiguous` plus the
      binding's handles) in `src/offline/driver.rs`, beside the conflict pass
      (`itemize_*`), and carry it into the report.
- [x] `SyncReport` gains a warnings section (`src/sync/report.rs`), rendered in
      text and `--json`, naming the collection and every handle; add its
      `*Output` entry and the json_schema.rs registration. *(`SyncReport` is
      itself the command's output type, printed through `printer.out`; this
      crate has no json_schema.rs, so a field is the whole registration.)*
- [x] Re-report it on every run, as `conflicts` already are, and word it as an
      ambiguity neverest will not resolve, never as an invalid mailbox.
- [x] Fix the silent write: an append performed by the sync appears as a hunk,
      and `already in sync` means nothing was written (seen in step 3 of the
      proposal, where a resurrected message was appended with an empty report).
      *(The unreported write is the relay: it streams the body server to server
      and never reaches the projection the report is built from, so every
      relayed copy was invisible, not only a resurrected one.)*
- [x] Unit tests: the warning renders in both formats with its coordinates; a
      run that appends reports a hunk.
- [x] `tests/duplicates.rs`, ignored by default like the other live tests,
      against `tests/stalwart2.sh`: seed one copy on A and two on B, sync,
      assert no hunk for that identity and a warning naming both UIDs; delete
      the bound copy on B, sync, assert A's copy survives and no delete is
      pushed; drop the right side's checkpoint, sync, assert nothing is appended
      to A.
- [x] `cargo test --all-features`, `cargo clippy --all-features --all-targets`,
      `cargo fmt`. *(clippy and fmt clean; 70 unit tests green and one failing,
      see the blocker below.)*
- [x] Fold `delta.md` into `cairn/spec/sync.md`; add the `cairn/log` entry; mark
      the change `landed` and archive it. *(Spec folded and log written; the
      status stays `active` until the blocker below is cleared, since the suite
      is not green.)*

## Addendum: unblocking the first task

io-replica and io-pimdir carry the freeze but neither is published, so the first
task above cannot be done as written. Instead:

- [x] `[patch.crates-io]` in neverest's Cargo.toml pointing `io-replica` and
      `io-pimdir` at `../io-replica` and `../io-pimdir`. The version
      requirements in `[dependencies]` stay honest, and the override is one
      block to delete once both are released.
- [x] Adapt to the engine API those releases carry, which the freeze came with:
      `ReplicaStorage::load` takes a `ReplicaLoadScope`, `ReplicaWriteOp::DropPlacement`
      carries a `ReplicaDropReason`, `ReplicaChange::Remove` carries a `link_id`,
      `ReplicaYield::WantsLoad` is a struct variant, and `ReplicaCollection` is gone.
      *(Plus io-pimdir's own in-flight `sourceless-store-handle`, which the same
      working tree carries and cannot be taken apart from the freeze:
      `PimdirStore::open(dir)` names no source, `for_source` yields the sync
      seam, so the driver's per-side handles are `PimdirSourceStore`.)*
- [x] The live test needs two Stalwart instances (`tests/stalwart2.sh`) and is
      `#[ignore]`d like the other live tests, so it is written but unproven
      until someone runs it against them. *(Run: it passes against both
      instances, so the whole chain is proven end to end rather than assumed.)*

## Blocker: the rekey is frozen by io-pimdir's floor

- [ ] Upstream, in io-pimdir: `save_bindings_diff` compares the hub before and
      after a whole write batch, so a rekey's `DropPlacement { Superseded }`
      plus its upsert under the new handle collapse into "same source,
      different handle". The freeze floor then keeps the old handle and records
      the new one as ambiguous, so a UIDVALIDITY bump freezes every item of the
      collection instead of carrying it over. io-replica states that batch is
      order-insensitive and marks the drops superseded precisely so a storage
      can tell a renumbering from a delete; the diff has to carry that knowledge
      down to the binding, not only to the item. `a_rekey_carries_state_by_link_id_and_bumps_the_generation_once`
      is the failing witness here.
- [ ] Once it is fixed: re-run the suite, mark this change `landed` and archive
      it.
- [ ] Drop the `[patch.crates-io]` block once io-replica and io-pimdir publish
      (the same tail `retention-sweep` and `submit-intent` still carry).
