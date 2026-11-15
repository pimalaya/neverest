---
cairn: log
change: the-command-line-is-the-familys
date: 2026-11-15
---

# The command line is the family's

Lines N2, N3, N11 and N12 of the CLI alignment round of 2026-11-15 (audit findings F20, F21, F27, F43, F45, F46).

## What landed

- src/cli/main.rs: `-c` is pimalaya-cli's `ConfigFlags` (one path per flag, repeatable, never split); `NEVEREST_CONFIG` is no longer read. `-a` stays neverest's own, repeatable. The help carries `footer!`; `completion`, `manual`, `json-schema` lose `arg_required_else_help` and come in himalaya's order. `meet_bare_invocation` and `welcome` as himalaya's.
- src/main.rs: a `ConfigMissingError` meets a person at a terminal with pimalaya-cli's welcome (`neverest check` the first command), a script or a JSON caller the error; a bare run fails without a configuration, prints the help with one.
- src/config.rs: `Config::load` is pimalaya-config's `TomlConfig::load` ("start from the sample:"); an unknown account reads "Account `x` not found, the configuration holds: a, b", still coded `account-missing`; `NO_CONFIG_HINT` is gone.
- Cargo.toml: clap with himalaya's feature set (`derive`, `error-context`, `help`, `std`, `suggestions`, `usage`); the lock drops anstream, colorchoice, terminal_size and their dependencies.
- tests/first_run.rs (10 tests) and tests/exit.rs (6 tests, tests/usage.rs folded in). Against the binary before the change, the colon test, the `NEVEREST_CONFIG` test, the wording tests, the bare run and the standard commands failed.
- .github/workflows/tests.yml: the `features` job, clippy over every backend alone and a reduced test build.
- README.md, config.sample.toml: the shared `-c`, the layered default paths, the welcome; README's empty code span for the Linux state directory now names $XDG_STATE_HOME.

## Capabilities moved

- [sync](../spec/sync.md): *A bare invocation prints the help* removed for *A first run meets the family's welcome*; *The command line is the family's* added (the no `repl` and no `mcp` sentence); *`-a` filters the accounts a command runs over* amended.
