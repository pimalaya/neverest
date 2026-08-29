---
cairn: log
change: both-endpoints-report-what-they-parked
landed: 2026-08-29
---

# Both endpoints report what they parked

The other half of the report bug found the same day, and the bigger half. `itemize_pulled` had exactly one caller, the single-source spine. `reconcile_pass`, which is the whole of the two-endpoint reconcile, ran each side's sync and asked its two reports only whether anything had moved; the per-item events went nowhere. So an account naming a source and a target reported no conflict at all: nothing in the text report, nothing in `--json`, and no notification, since the announcement is raised from a list nothing ever filled. That is the topology someone mirroring two servers runs, so the feature was missing exactly where it matters most, and the worker-barrier fix landed earlier the same day did not reach it. The outstanding count was right throughout, being read from the store, so such a run did say how many decisions were waiting, and never which.

`itemize_conflicted` is now split out of `itemize_pulled` and called by both paths: the events say which placements entered conflict, the store says which survived the run's own merge, and only those are reported. `reconcile_pass` calls it for each side right after that side's merge and before the other side runs, which is the only place the events still exist. `sync_collection` fills a report of its own and folds it through `SyncReport::absorb`, so both topologies now go through the one destructuring that names every field, rather than the pair path appending straight into the account's.

The convergence loop was the part worth thinking about. A collection is reconciled up to five times and both endpoints write into one report, so a naive fix multiplies. Three separate things keep it honest. The engine says nothing about a placement it has already parked, so the repeat that actually happens is the run's own merge settling a divergence and a later pass marking it again after the remote moved once more: `SyncReport::note_conflict` records it once, keyed on the endpoint, the collection and the item. The outstanding count still comes from the store, because "what entered conflict during this run", which the notification keys on, and "what is waiting now", which the exit code answers, are different questions and neither is derivable from the other. And a refusal re-observed on a later pass is still one refusal: a create the other side will not take is re-derived and re-refused every pass, so `RefusedCreate` gained the handle it was appended under, which is what tells one copy from the same copy meeting the same answer again, two copies of one identity being genuinely two refusals.

The test drives the engine in the two-endpoint shape, two source stores over one hub against two fake mutable remotes, for two passes in the order `reconcile_pass` runs them, and settles the left side between passes the way the merge does so the second pass genuinely marks it again. Without the dedup it reports three lines for two divergences, which is what a naive fix ships. It also asserts the announcement: two items entering conflict announce twice, and a later run over the same parked placements announces nothing, which is the once-only rule the spec states. `announce_conflicts` returns how many it announced so that assertion is on the function rather than on a reading of it.

Left alone: the pulled flag and delete hunks, which the two-endpoint path still does not itemize. It reports the cross-endpoint propagation plan instead, which is a different account of the same run, and making both topologies report both would double-count against the projection hunks. Flagged in the proposal.

Capabilities moved: sync, one modified requirement on the report.
