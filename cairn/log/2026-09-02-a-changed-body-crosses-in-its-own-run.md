---
cairn: log
change: a-changed-body-crosses-in-its-own-run
landed: 2026-09-02
---

# A body changed on one endpoint crosses in the run that observed it

A mirror is what `targets` is for, and an item edited on one endpoint was not handed to the other by the run that saw the edit. With `retain = true` it landed a run late, reported as applied by both runs. With `retain` unset, the default as soon as targets are declared, it never landed at all: six runs against two CardDAV principals, the same `{"kind":"update","side":"b","error":null}` every time, exit 0 every time, the two servers permanently divergent.

## The pass that hydrates nothing

`hydration_targets` returned the one-sided case only: an item exactly one source holds, with no shared body, to be fetched so the other side's create can push it. An item both endpoints hold whose body one of them changed looks almost the same, since recording a remote content change drops the stale body from the item and from that source's base together, so the hub holds no shared body for it either. It was skipped for having two bindings.

Nothing else fetched it inside the pass, so the pushing passes had no body to send, while `itemize` derived the update from the projection regardless. Under `retain = true` the account-wide hydration filled the body after the collection was done, which is why the following run could push and why the defect read as a one-run lag rather than as a permanent one.

The conflict path was never affected, and that is the tell: `resolve_conflicts` hydrates inside the pass, so a card edited on **both** endpoints converged in one run while a card edited on one did not. The cheap case was the broken one.

## What landed

**[hydration_targets](../../src/offline/storage.rs) takes a second shape**: an item both endpoints hold, with no shared body, whose base body exactly one of them lost. That one is the changed side, and its body is fetched to `Full` during the propagate pass, so the pushing passes read the other endpoint as dirty against it and deliver it in the same run.

**A divergence stays the conflict path's**, and is told apart by the same reading: both bindings having lost their base body is both endpoints having rewritten the item, which merges or parks. Unless an authority is declared, where there is no divergence to reconcile at all: `HydrationSide::decides` carries that, and the deciding side's body is hydrated so `one-way` overwrites in the run that saw the difference. The one-way live test failed for exactly this reason before the second leg went in.

**The permission read moved with the shape**: a copy is gated on the far side's `item.create` as before, an update on its `item.update`, which is what the write would be.

## Tests

`a_one_way_account_overwrites_the_target_instead_of_parking_the_divergence` in [tests/endpoints.rs](../../tests/endpoints.rs) is the regression witness: written as a failing test against the defect, passing now, and its ignore reason back to naming only the server it needs.

The live reproduction was rerun in both `retain` modes from a clean store: the update lands in the run that observed it and the run after is quiet. The whole live suite, twenty tests over nine binaries, is green.

## Capabilities moved

- sync: a body changed on one endpoint reaches the other in the run that observed it, whatever `retain` says
- sync: a run reports no write it did not attempt
- sync: `one-way` overwrites in the run that saw the difference rather than the one after
