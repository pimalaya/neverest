---
cairn: tasks
change: wizard-removed
---

# Tasks

- [x] pimalaya-cli 0.4 through the local path patch; the two moved imports
- [x] src/wizard/ and src/cli/configure.rs removed, with the `wizard` feature, io-pim-discovery and every `cfg(feature = "wizard")` site
- [x] A bare `neverest` prints the help; `Config::load` fails naming the path and the sample
- [x] The wizard-only helpers in src/config.rs and src/account.rs removed, with the render test
- [x] README, config.sample.toml, CONTRIBUTING, MIGRATION, CHANGELOG (BREAKING, Removed)
- [x] Spec folded; log written
- [x] `cargo fmt`; `cargo clippy --all-targets -- -D warnings` with default features, `--all-features` and `--no-default-features`; `cargo test` with default features and `--all-features`
- [ ] The first-run welcome (pimalaya-cli `welcome::Welcome`), after the move to pimalaya-config 0.4
