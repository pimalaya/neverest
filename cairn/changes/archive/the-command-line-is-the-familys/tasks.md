---
cairn: tasks
change: the-command-line-is-the-familys
---

# Tasks

- [x] `-c` through `ConfigFlags`, `NEVEREST_CONFIG` gone (src/cli/main.rs)
- [x] `Config::load` through pimalaya-config's `TomlConfig::load`; the family wording for a missing configuration, no account and an unknown one (src/config.rs)
- [x] The welcome in `main`, a bare run through `meet_bare_invocation` (src/main.rs, src/cli/main.rs)
- [x] `arg_required_else_help` off the standard commands, himalaya's order, `footer!`, clap's feature set (Cargo.toml)
- [x] tests/first_run.rs and tests/exit.rs, tests/usage.rs folded in; checked against the binary before the change (colon split, `NEVEREST_CONFIG` read, old wording)
- [x] CI `features` job (.github/workflows/tests.yml)
- [x] README, config.sample.toml, CHANGELOG, MIGRATION.md
- [x] Fold into cairn/spec/sync.md, log, archive
