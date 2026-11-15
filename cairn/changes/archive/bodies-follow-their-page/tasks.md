---
cairn: tasks
change: bodies-follow-their-page
---

# Tasks

- [x] `run_verb_paged` and the `Pager` trait (src/offline/mod.rs).
- [x] `HydrateFeed`, `PageFeed`, per-collection gates, idle workers draining the feed in phase 1; phase 2 targets read after phase 1 (src/offline/driver.rs).
- [x] `itemize_fetches` names what was bodiless before the pull.
- [x] io-pimdir `f9b13f8`.
- [x] Tests: unit (a page told at rest keeps its bodies through later pages, a resumed round feeds what its earlier pages owe, the feed waits for spines); tests/scope.rs on Stalwart (bodies before the round closes, interruption keeps pages and bodies, the next run fetches the rest); hydration and the other Stalwart suites.
- [x] Spec folded, log written, CHANGELOG.
