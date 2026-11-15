---
cairn: change
id: a-changed-body-crosses-in-its-own-run
status: landed
created: 2026-09-02
---

# A body changed on one endpoint crosses in the run that observed it

## Why

An account with a `targets` table mirrors two endpoints, and an item edited on one of them was never handed to the other by the run that saw the edit.

Reproduced against two CardDAV principals, one source and one target, from a clean store. The card crosses correctly on the first run. Edit it on the source and sync again: the report carries `{"kind":"update","side":"b","error":null}`, the run exits 0, and the target still holds the old body. With `retain = true` the next run pushes it and reports the same update a second time. With `retain` unset, which is the default as soon as `targets` is declared, no later run ever pushes it: six runs, the same hunk reported every time, the two endpoints permanently divergent while every run claims the write.

That is the shape the spec already refuses elsewhere. A refused write is taken back so a run that wrote nothing does not read as having written; here a write nothing even attempted was counted as applied.

The cause is that the propagate pass hydrates only what a create needs. `hydration_targets` skipped any item held by more than one source, because it was written for the one-sided case: a body one endpoint holds and the other must be given. An item both endpoints hold, whose body one of them pulled a change to, holds no shared body either (recording a remote content change drops the stale body from the item and from that source's base together), and it was skipped. The push then had nothing to send while the itemizer still derived the update from the projection. Under `retain = true` the account-wide retain hydration filled the body afterwards, which is why the run after could push; under `retain = false` nothing ever fetched it.

The conflict path never had the defect because `resolve_conflicts` hydrates inside the pass, which is why a card edited on both endpoints converges in one run while a card edited on one does not.

## What changes

`hydration_targets` gains the second shape. An item both endpoints hold, with no shared body, whose base body exactly one of them lost, is that endpoint's body to hand over: it is fetched to `Full` during the propagate pass, so the pushing passes that follow read the other endpoint as dirty against it and deliver it in the same run.

An item both of them rewrote is not that: it is a divergence, and the conflict path merges or parks it. Unless an authority is declared, in which case there is no divergence to reconcile and the deciding side's body is what crosses, which is the same hydration under a different reason. `one-way` therefore overwrites in the run that observes the difference rather than the run after.

The permission read moves with the shape. A copy is gated on the far side's `item.create`, as before; an update is gated on its `item.update`, which is what the write would be.

## Out of scope

No CHANGELOG entry: the defect only ever existed in code that has not shipped, so it is interior churn to the 1.0.0 section, which reports the net change from v1.0.0-beta.
