---
cairn: tasks
change: a-refused-line-exits-two
---

# Tasks

- [x] `pimalaya_cli::clap::parse::<Cli>()` in src/main.rs; no other clap error becomes a failure
- [x] `Exit::Conflicted` exits 3 and `Exit::Incomplete` 4; `check` exits 3 through `PartialOutcome::default()`
- [x] Doc comments, the `sync` help, unit tests (src/cli/exit.rs) and the integration tests renumbered
- [x] tests/usage.rs: the `usage-invalid` object and exit 2, with and without `--json`
- [x] README, config.sample.toml, MIGRATION.md, CHANGELOG (BREAKING)
- [x] Spec folded; log written
- [x] `cargo fmt`, clippy and tests (see the log)
- [ ] The server suites (Stalwart, Radicale, live) on the new codes: the owner
