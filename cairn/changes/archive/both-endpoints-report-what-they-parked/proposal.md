---
cairn: change
id: both-endpoints-report-what-they-parked
status: landed
created: 2026-08-29
---

# Both endpoints report what they parked

## Why

`itemize_pulled` was called from one place, the single-source spine. `reconcile_pass`, which is the whole of the two-endpoint reconcile, ran each side's sync and then asked its two reports one question:

    Ok(moved(&left_report) || moved(&right_report))

The events went nowhere. **For an account naming a source and a target, a conflict was never reported at all**: not in the text report, not in `--json`, and `conflict.notify` could not fire there either, the announcement being raised from a list nothing ever filled. That is the topology someone mirroring two servers runs, so the feature was absent exactly where it matters most, and the `a-run-names-what-it-parked` fix, which repaired the worker-barrier merge of the *other* path, did not reach it.

The outstanding count was right throughout, being read from the store, so a two-endpoint run did say how many decisions were waiting. It never said which, in which collection, on which endpoint, and it never announced one.

## What

- `itemize_conflicted` is split out of `itemize_pulled`: the events say which placements *entered* conflict, the store says which of them survived the run's own merge, and only those are reported. Both paths call it, so there is one answer to "what did this run park".
- `reconcile_pass` calls it for each side, right after that side's own `resolve_conflicts` and before the other side runs, which is the only place the events still exist.
- `SyncReport::note_conflict` records a divergence unless the run named it already, keyed on the side, the collection and the item.
- `itemize_refused` and `itemize_rejected` drop the repeats their batch collects for the same reason.
- `sync_collection` now fills a report of its own and folds it into the account's through `SyncReport::absorb`, the same fold the one-source path makes, rather than appending to the account report directly. Both paths therefore go through the one destructuring that names every field.

## The convergence loop, which is the hard half

A collection is reconciled until it is quiescent, so a pass runs up to five times over one collection and both endpoints report into one report. Three things keep the count honest, and they are not the same mechanism:

- **The engine is silent about a placement it has already parked.** So a naive second pass usually adds nothing by itself, and the repeat that does happen is the interesting one: the run's own merge settles a divergence, a later pass pushes the merged body, the remote has moved again, and the placement is marked a second time. That is one divergence and one line, and it is what `note_conflict` is for.
- **The outstanding count still comes from the store**, not from this list. The two answer different questions: what entered conflict during this run, which is what the notification keys on, and what is waiting now, which is what the exit code answers. Neither is derivable from the other.
- **A refusal re-observed on a later pass is still one refusal.** A create the other side will not take is re-derived and re-refused on every pass, so the drained batch carries it once per pass. Two *copies* of one identity are genuinely two refusals, so `RefusedCreate` now carries the handle it was appended under: that is what tells one copy from the same copy meeting the same answer again. The handle is a key and not something the report says, the two copies sharing everything a person would act on.

## Not in scope

**The pulled flag and delete hunks.** `itemize_pulled` also turns `FlagsChanged` and `Vanished` into hunks, and the two-endpoint path still does not. It reports the cross-endpoint propagation plan instead, which is a different account of the same run and not obviously wrong; making both topologies report both would double-count against the projection hunks `itemize` already derives. Conflicts are what was missing and what this change adds.
