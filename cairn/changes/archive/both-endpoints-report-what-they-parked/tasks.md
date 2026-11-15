---
cairn: tasks
change: both-endpoints-report-what-they-parked
---

# Tasks

- [x] Split `itemize_conflicted` out of `itemize_pulled`.
- [x] Itemize each side per pass in `reconcile_pass`.
- [x] Note a divergence once, however many passes and endpoints marked it.
- [x] Drop the repeats a convergence loop collects for refusals and rejections.
- [x] Fold the two-endpoint path through `SyncReport::absorb` too.
- [x] Drive it offline in the two-endpoint shape, over more than one pass.
- [x] CHANGELOG.md.
- [x] Fold the delta into cairn/spec/sync.md and log it.
