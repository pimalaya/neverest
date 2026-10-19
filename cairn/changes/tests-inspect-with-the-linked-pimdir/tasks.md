---
cairn: tasks
change: tests-inspect-with-the-linked-pimdir
---

# Tasks

- [ ] A shared test helper (tests/common or a module) reading items, retention and the other states the suites assert through the io-pimdir library neverest links.
- [ ] tests/caldav.rs:255, tests/carddav.rs:615, tests/calendars.rs:520, tests/submit.rs:422: the `pimdir` helpers moved onto it, `Command::new("pimdir")` gone.
- [ ] Assertions matched to the library's rendering where it differs from the CLI's `--json`.
- [ ] The Radicale suites green on a machine with no `pimdir` on PATH.
- [ ] `cargo test`, clippy (default, `--all-features`); log written.
