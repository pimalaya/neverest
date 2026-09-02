---
cairn: change
id: a-divergence-with-no-ancestor-parks
status: landed
created: 2026-08-30
---

# A divergence the two endpoints never agreed on parks instead of resolving itself

## Why

Two endpoints holding one identity under two different bodies, before either has been read, is not a rare shape: it is what a migration between two providers starts from, and what a mirror looks like the day someone adds the second server by hand. The account whose endpoints changed a card differently *after* they agreed on it is now handled, three-way merged against the body they came from and parked where the merge collides. The account whose endpoints never agreed on anything was not, and it lost data on the very first run.

Seed `UID:card-1` with `TEL:+1` on one CardDAV principal and the same `UID` with `TEL:+9` on the other, then init and sync: the run reports `outstanding_conflicts: 0`, one `update` hunk on the target, exits 0, and both servers end up holding `+1`. The `+9` the target had held is gone, replaced by a body nobody chose over it. Reporting the hunk is not consent: the run named a write it should never have derived.

The engine already saw it. The hub reconciles two axes, one per source against its own remote and one across sources, and the second is exactly this case: the source folds its body in as the shared one, the target arrives with a different body and no prior binding, and with nothing behind the two the hub flags the *item* `conflicted` and keeps the target's body in `conflict_object`. Nothing in neverest read either field. What it did read is the per-source flag on the binding, which nothing here sets, so the target projected as ordinarily dirty against the shared body and the next round pushed it.

The two axes are genuinely different questions, "this endpoint and its server disagree" against "the two endpoints disagree", and a two-endpoint account needs both answered. Only one was.

## What

- Read the hub's cross-source divergences after each reconcile round, and park the ones the round itself recorded: the target's placement is written `Conflict`, carrying the source's body as its own and the target's as the divergence against it, at the revision the target's own pull observed.
- Do not merge. There is no common ancestor to merge against, the two servers never having agreed on anything, so the parked placement carries no base body and the automatic merge leaves it alone rather than settling it on whichever side it happens to read as unchanged.
- Take the divergence before the pushing round, so marking it is what stops the push: the hub projects a conflicted binding as `Conflict` and never as the `Dirty` a round would push, which is the data-loss fix and not only the report.
- Report it the way every other parked divergence is reported: named once by the run, counted among the outstanding, exited 2 on, listed by `conflict list` and settled by `conflict resolve` either way round.
- Read the flag as an edge against a snapshot taken before the round. The hub keeps it on the item once set, no upsert clearing it where the source restates the shared body, so parking on the flag alone would re-park a divergence a person had already settled and refuse the resolution's own push forever.
- Leave `one-way` and dry runs alone, the same bar the ancestor case holds: under a declared authority the source decides and a difference between the two is not a divergence.
