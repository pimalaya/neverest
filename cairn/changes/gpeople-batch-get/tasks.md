---
cairn: tasks
change: gpeople-batch-get
---

# Tasks

- [ ] io-gpeople: check 0.5 for `people:batchGet`; if missing, add it (`resourceNames` up to 200, `personFields`, per-person status) and release it.
- [ ] src/gpeople/client.rs: `fetch_bodies` (lines 291-310) over the batch get, chunks of 200, each person committed as read; a not-found resource left out as gone; `person` (line 284) kept only where one person is read alone.
- [ ] Tests: a scripted session answering a batch of 3 with one not found; the call count for 450 ids is 3.
- [ ] tests/google.rs: the cleanup (lines 157-183) deletes the run's cards by resource name, `contacts_search` (line 170) only as a fallback.
- [ ] Spec folded, log written, CHANGELOG.
- [ ] `cargo test`, clippy (default, `--all-features`).
- [ ] Live: `google_contacts_sync_both_ways` right after the io-gpeople live suite, no 429.
