---
cairn: tasks
change: a-refusal-carries-its-code
---

# Tasks

- [x] pimalaya-cli 0be1b1f, with the io-* and pimalaya-stream 0.4 bump (c1837c5)
- [x] `permission-denied`, `quota-exceeded`, `rate-limited`, `recipient-refused` in src/error.rs
- [x] `Failure`, `error::code` and the chain walk over one reader per protocol: DAV, Graph, Gmail, Google Calendar, Google People, IMAP, SMTP
- [x] `code`, `error` and `sources` on the patch entries, `rejected` (`reason` renamed), `submitted`, `intents`, `parked` and `unreached`, and drain's `parked`
- [x] Account rows (`sync`, `drain`, `init`) coded by `error::code`; the index refresh coded by the store
- [x] Tests: a reader per protocol through its client's scripted server (DAV, IMAP, Graph, Gmail), Google's codes, the driver's rejected row in JSON and text, a refused recipient and a sending limit end to end (tests/intents.rs)
- [x] Spec folded; log written; CHANGELOG and MIGRATION.md
- [x] `cargo fmt`, clippy over every backend alone, default, none and all; `cargo test --all-features`
- [ ] The server suites (Stalwart, Radicale, live) on the new rows: the owner
