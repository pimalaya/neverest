---
cairn: tasks
change: gpeople-batch-get
---

# Tasks

- [x] io-gpeople: check 0.5 for `people:batchGet`; if missing, add it (`resourceNames` up to 200, `personFields`, per-person status) and release it. Released io-gpeople 0.8.1, which neverest locks, has `GpeoplePersonsBatchGet` (checked 2026-11-19).
- [x] src/gpeople/client.rs: `fetch_bodies` (lines 291-310) over the batch get, chunks of 200, each person committed as read; a not-found resource left out as gone; `person` (line 284) kept only where one person is read alone.
- [x] Tests: a scripted session answering a batch of 3 with one not found; the call count for 450 ids is 3.
- [ ] tests/google.rs: the cleanup (lines 157-183) deletes the run's cards by resource name, `contacts_search` (line 170) only as a fallback. Left on 2026-11-19: the cleanup runs after `round_trip`, whose replicas and their bindings are gone by then, so recording the resource names means changing `round_trip` for every backend; a live suite change nobody can run offline, left with the live line below. Not planned (2026-11-26): the fallback cleans up, and the live nights since left no card behind (.lanes/live/report-3.md §6).
- [x] Spec folded, log written, CHANGELOG.
- [x] `cargo test`, clippy (default, `--all-features`).
- [x] Live: `google_contacts_sync_both_ways` right after the io-gpeople live suite, no 429. The live night of 2026-11-25 ran tests/google.rs `--ignored` on the 1.0 candidate, 8 of 8 passing, this test among them (.lanes/live/report-3.md §2), though not right after io-gpeople's suite.
