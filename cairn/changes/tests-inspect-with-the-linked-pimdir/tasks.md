---
cairn: tasks
change: tests-inspect-with-the-linked-pimdir
---

# Tasks

- [x] A shared test helper (tests/common or a module) reading items, retention and the other states the suites assert through the io-pimdir library neverest links.
- [x] tests/caldav.rs:255, tests/carddav.rs:615, tests/calendars.rs:520, tests/submit.rs:422: the `pimdir` helpers moved onto it, `Command::new("pimdir")` gone. tests/endpoints.rs moved too.
- [x] Assertions matched to the library's rendering where it differs from the CLI's `--json`.
- [ ] The Radicale suites green on a machine with no `pimdir` on PATH. Not run on 2026-11-19: the flake provides no Radicale (tests/radicale.sh runs a Docker image), so the five suites were built and not run; submit.rs needs Stalwart (tests/stalwart2.sh) as well.
- [x] `cargo test`, clippy (default, `--all-features`); log written.
