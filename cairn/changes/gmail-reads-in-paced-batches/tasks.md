---
cairn: tasks
change: gmail-reads-in-paced-batches
---

# Tasks

- [x] Throttle: Google refusals by reason (`GoogleRefusal`, `google_refusal`, `GoogleWaits`), Android's three real bodies verbatim; Pacific midnight; the minute's sliding budget.
- [x] gcal and gpeople read through the same classification (gcal by its message).
- [x] `src/gmail/batch.rs`: `read_all` over a `Batcher`, ported from Android with its fake batcher tests (batches of ten, refused put back at the head, minute and daily, six tries, raw size caps).
- [x] `src/gmail/run.rs`: settings and gate, metadata and size caches, the run's history and its projection, with tests.
- [x] `src/gmail/client.rs`: round pages, deltas, summaries and bodies over batches; history once a run; writes on a gone message converge; scripted-session tests.
- [x] Config: `gmail.batch-size`, `gmail.batches-in-flight`, `gmail.units-per-minute`, refused out of range; config.sample.toml.
- [x] Spec folded, log written, CHANGELOG.
- [x] `cargo test`, clippy (default, `--all-features`, MOA's set, reduced sets).
- [ ] Live: a first sync and a delta of google@pimalaya.org (`tests/google.rs`), counting batches and refusals.
