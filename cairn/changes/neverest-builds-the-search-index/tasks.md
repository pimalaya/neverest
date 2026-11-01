---
cairn: tasks
change: neverest-builds-the-search-index
---

# Tasks

- [x] Wait for io-pimdir's modified `search` API (query-only open, refresh with a deadline, typed errors); pin it.
- [x] `search` feature of io-pimdir enabled, on by default; the ical-rs pin io-pimdir's search needs.
- [x] Refresh after sync and after drain, under a deadline (`store.index-timeout`); an index error reported, never failing the run.
- [x] Full-build command (`neverest index`).
- [x] Tests: a refresh after a run indexes what it landed, as a query-only open reads it; a refresh past its deadline resumes next run; an index failure is reported, not raised; a zero timeout refreshes nothing; an index error leaves the exit code; a reset drops or moves the index with the store.
- [x] Spec folded, log written, CHANGELOG, README.
