---
cairn: change
id: neverest-follows-sync-draft-05
status: landed
created: 2026-10-30
---

# neverest follows sync-draft-05

## Why

Phase A of the pimdir stable-release plan, part 2a: io-pimdir's local tree landed the store side of storage-draft-05 and the engine of sync-draft-05 (its logs the-store-follows-storage-draft-05, the-engine-follows-sync-draft-05, the-agreed-body-stays-pinned, a-queued-edit-names-what-it-changes, a-listing-proves-what-it-drops), breaking the seam neverest drives. The flag deltas, the mass-absence guard and the pinned agreed body have changes of their own (a-flag-push-names-what-it-changes, a-mass-absence-waits-for-a-person, the-engine-pins-the-agreed-body); this one is the rest of the port.

## What

- A queued `update` and an engine `Edit` state the body they were made over (`if_match`): a conflict resolution the local side it was decided on, a merge the local side it merged, a one-way overwrite the body it restates. One replaced since parks rather than being dropped unmerged.
- A body staged with its action goes through `PimdirProducer::enqueue_with_body`, so a refused action leaves no body behind (the conflict resolution, the merge, the test writers).
- The runner answers `PimdirYield::WantsVerify`, through the store, and profiles it.
- A change withheld over an unreadable body (`PimdirSyncReport::unreadable`) is reported under `unreadable` and waits for a person (exit 2): the store needs a repair a rerun does not make.
- A body a policy did not adopt (`PimdirSyncEvent::Overruled`, kept as a retained item `dup:<link>#<handle>`) is reported under `overruled`, a warning that changes no exit code.
- A stated identity filed escaped (`esc:`, STORAGE §9) is named to a server without its escape, minted or not, as `PimdirLinkId::stated` does.
- A stored body streams through io-pimdir's verifying reader (`PimdirBlobReader`).
- tests/drain.rs runs with the `dav` feature only, so a single-backend test build passes.
- Test fixtures name bodies by their real hash, a body now being checked against its name before a push or an upgrade.
