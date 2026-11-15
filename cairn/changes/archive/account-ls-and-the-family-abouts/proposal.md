---
cairn: change
id: account-ls-and-the-family-abouts
status: landed
created: 2026-11-15
---

# `account ls` and the family's abouts

## Why

Rows L12 and L10 of the X3 check of the CLI alignment round of 2026-11-15 (pimalaya/.github cairn/changes/cli-alignment-2026-11-15/reports/x3-2026-11-15.md), line X7.

- **L12**: `neverest account ls` was refused (exit 2), where himalaya, ortie, cardamum, calendula and carillon all take `ls` as a visible alias of `account list`.
- **L10**: the `account` group opened on "Inspect the accounts the configuration declares", where himalaya, ortie and cardamum read "Manage accounts defined in the TOML configuration file", and `account list` on "List the accounts the configuration declares", where himalaya reads "List all accounts declared in the configuration".

## What

- src/cli/account.rs: `#[command(visible_alias = "ls")]` on `AccountCommand::List`, as himalaya's src/account/cli.rs carries it; the group's about is himalaya's first line, neverest's own paragraph (sources over one pimdir store, written by hand, `-a` filtering) kept after it; `account list` takes himalaya's first line, its own paragraph kept.
- tests/account.rs: `account_ls_lists_and_the_abouts_are_the_familys`, `account ls` printing what `account list` prints, `account --help` opening on the new about with neverest's paragraph after it, `account -h` listing `list` with `[alias: ls]`. It fails on the code before (exit 2).
- cairn/spec/sync.md, "The account group is the family's": `ls` named as `account list`'s visible alias, a scenario for it.
- CHANGELOG: one line under Changed.

## Decisions

- **Visible, as in the five others.** The alias shows in the help as himalaya's, ortie's, cardamum's, calendula's and carillon's do; the help of a family binary lists the same names.
- **The abouts are not specified.** The spec names commands and outputs, not help wording, so only the `ls` sentence moves.
- **`account check` keeps its about** ("Probes every configured source before a real sync"): row L10 names the group and its list only.
