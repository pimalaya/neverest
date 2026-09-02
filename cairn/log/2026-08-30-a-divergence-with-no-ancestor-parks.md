---
cairn: log
change: a-divergence-with-no-ancestor-parks
landed: 2026-08-30
---

# A divergence the two endpoints never agreed on parks instead of resolving itself

Two endpoints of one account changing a card differently after they agreed on it is now merged or parked. Two endpoints holding one identity under two different bodies *before* they ever agreed was not, and it lost the target's body on the first run.

Against a Radicale with two principals: `UID:card-1` with `TEL:+1` on one and the same `UID` with `TEL:+9` on the other, then init and sync. The run reported `outstanding_conflicts: 0`, one `update` hunk on the target, exited 0, and both servers ended up holding `+1`. Nothing named the `+9` that had been there. This is not a rare shape, it is what a migration between two providers and a hand-built mirror both start from.

## The axis nobody read

The hub reconciles two axes and neverest read one of them. `ReplicaSourceBinding::conflicted` says an endpoint and its own server disagree, and `hub.project` turns it into a `Conflict` placement, which is the divergence the one-endpoint path has always handled. `ReplicaHubItem::conflicted` says the two endpoints disagree, and it fires on exactly this path: the source folds its body in as the shared one, the target arrives with a different body against no prior binding, and with nothing behind the two the hub keeps the shared body, sets the flag and records the target's body in `conflict_object`.

Verified on the repro before anything was written: `items.conflicted = 1` and `items.conflict_object` holding the target's own body, with both bindings unconflicted. The flag was right and nothing was reading it, so the target projected as ordinarily dirty against the shared body and the pushing round pushed it.

The timing matters as much as the reading. A run's opening round pulls and never pushes, and the flag is set inside it, when the target's own pull is absorbed. Taking the divergence at the end of that round is what makes marking the target a fix rather than a report: the hub projects a conflicted binding as `Conflict` and never as the `Dirty` the pushing round would have pushed.

## What landed

**[hub_divergences](../../src/offline/driver.rs) snapshots the flag before the round**, beside `shared_bodies` and read for the mirror-image reason. The hub keeps the cross-source flag on the item once set: a source restating the shared body moves neither axis the flag is cleared on, so it outlives the decision that settled it. Parking on the flag alone re-parked what `--prefer-local` had just settled, refusing the resolution's own push on every run after it, which the repro showed before the snapshot went in.

**`hub_conflicts` reads what the round itself recorded**, an item flagged with a diverging body the snapshot did not already carry, both endpoints bound, and the target not already holding a decision of its own.

**`park_hub_conflicts` writes the target's placement `Conflict`**: the source's body as the item's own, the target's in `conflict_object`, and the revision the target's pull observed as both the base revision and the conflict revision. Its base carries no body, which is deliberate twice over: the two endpoints never agreed on one, and it is what keeps `merge_conflicts` out. Merging against the target's own body as the base would read the target as having changed nothing and settle on the source's, which is the overwrite the parking exists to refuse.

Nothing was added to the resolution path, the report path or the conflict commands, as with the ancestor case: the divergence is given the shape they already read. `--prefer-local` settles on the source's body, `--prefer-remote` on the target's, and one run after either carries the decision to both endpoints.

`parks_divergences` gates it as it gates the ancestor case: both endpoints authoritative, not a dry run. Under `one-way` the source overwrites the target by declaration and nothing parks, which a live run confirmed.

## Tests

`two_endpoints_holding_one_card_under_two_bodies_park_it_and_never_overwrite` in [tests/endpoints.rs](../../tests/endpoints.rs), against the two Radicale principals, in an address book of its own. It seeds two identities under two bodies each, checks the first run parks both, names both, pushes nothing and exits 2, that each server still holds its own body across a rerun, and then settles one with `--prefer-local` and the other with `--prefer-remote` and checks a single run converges both endpoints on both decisions.

With the park disabled it fails on the first run, exit 0 against an expected 2, with two `update` hunks on the target and `outstanding_conflicts: 0`, which is the defect verbatim.

## Capabilities moved

- sync: a divergence between two endpoints with no common ancestor parks, is reported and exits 2, and neither body is written over the other
