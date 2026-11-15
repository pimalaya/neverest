---
cairn: change
id: the-command-line-is-the-familys
status: landed
created: 2026-11-15
---

# The command line is the family's

## Why

The CLI alignment round of 2026-11-15 (pimalaya/.github cairn/changes/cli-alignment-2026-11-15, lines N2, N3, N11 and N12; audit findings F20, F21, F27, F43, F45, F46) found neverest's command line apart from the five other Pimalaya CLIs:

- F20: `-c` was neverest's own flag, a `:`-split list also read from `NEVEREST_CONFIG`; the others share pimalaya-cli's repeatable `-c`, one path per flag, no environment variable.
- F21: a bare `neverest` with no configuration printed the help and exited 0, where the others fail naming the missing file (the welcome at a terminal); `manual` and `completion` with no argument printed their help on stderr and exited 2 (`arg_required_else_help`), where the others run and fail on stdout, exit 1.
- F43: no first-run welcome, its own "write one by hand" wording for a missing configuration and "Cannot find account x, the configuration declares ..." for an unknown one, no help footer, clap with its default features.
- F27: the standard commands came in ortie's order, `manual` before `completion`.
- F45, F46: no tests/first_run.rs nor tests/exit.rs (the usage tests lived in tests/usage.rs), no CI `features` job, and nothing said why neverest has no `repl` and no `mcp`.

## What

- `-c` is pimalaya-cli's `ConfigFlags`; `NEVEREST_CONFIG` is no longer read. `-a` stays neverest's own, repeatable.
- A missing configuration fails through pimalaya-config's `ConfigMissingError` ("No configuration found at <path>, start from the sample: <url>"), which `main` meets with pimalaya-cli's welcome when stderr is a terminal and the output is text, as himalaya does. A bare `neverest` fails the same without a configuration and prints the help with one.
- `completion`, `manual` and `json-schema` lose `arg_required_else_help` and come in himalaya's order.
- An unknown account reads "Account `x` not found, the configuration holds: a, b"; a configuration holding no account points at the sample in the same words.
- The help ends with pimalaya-cli's `footer!`; clap takes himalaya's feature set.
- tests/first_run.rs and tests/exit.rs as himalaya's and carillon's, tests/usage.rs folded into the latter; the CI `features` job as himalaya's.
- The spec says why there is no `repl` and no `mcp`.

## Decisions

- **`-a` stays repeatable.** The shared `AccountFlag` takes one name; a neverest run covers several accounts, so its flag keeps `ArgAction::Append` (brief of 2026-11-15).
- **The order of the standard commands follows P5** (himalaya's), ruled for X1, since the enum was rewritten here anyway.
- **tests/usage.rs is folded into tests/exit.rs**, as himalaya, cardamum and carillon keep their usage tests.
