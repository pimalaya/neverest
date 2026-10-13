---
cairn: tasks
change: a-body-is-downloaded-once
---

# Tasks

- [x] `client`: `Gone`, `StoredSize`, `fetch_bodies` answering the gone ids and taking the stored sizes, `global_handles`.
- [x] `gmail`: gone ids from the size and raw passes, a lone 404 as `Gone`; the store's sizes after the run's, `minimal` only for the rest; scripted-session tests.
- [x] `msgraph`: gone ids from the size batch, the body batch and the lone reads (mail, contacts, events), a lone 404 as `Gone`; the store's sizes; scripted-session tests.
- [x] `imap`: a UID a completed `UID FETCH` answered nothing for is gone; a lone fetch streaming nothing is `Gone`.
- [x] `offline/remote`: `hydrate_batch` answers `Hydrated { items, gone }`; `stored_item` reads a stored body back as a fetch.
- [x] `offline/driver`: `HydrateBatch` carries sizes; `GoneBodies`; `Landing::land` shared by both phases (each connection's store handle in phase 2); `Placed` for account-wide handles; phase 2 plans a shared handle once.
- [x] Driver tests over a scripted Gmail: a message under two labels downloaded once and landed under both, a gone body owed nothing more in the run, no `minimal` read; a body held under one label serving another without a download.
- [x] Stalwart: three messages expunged while the first batch downloads; the run completes, their members stay as listed, and the next run's listing takes them away asking nothing.
- [x] Spec folded, log written, CHANGELOG.
- [x] `cargo test` (default, `--all-features`, MOA's set), clippy (the same and the reduced sets), the Stalwart suites touching hydration (hydration, scope, moves, drain, submit) and the Radicale ones (carddav, caldav).
- [ ] Live: a Gmail account with a message under two labels (`tests/google.rs`).
