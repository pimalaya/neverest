---
cairn: log
change: account-ls-and-the-family-abouts
date: 2026-11-15
---

# `account ls` and the family's abouts

Rows L12 and L10 of the X3 check of the CLI alignment round of 2026-11-15, line X7.

## What landed

- `account ls` lists the accounts as `account list` does, a visible alias as in himalaya, ortie, cardamum, calendula and carillon, where it exited 2 (src/cli/account.rs).
- The `account` group opens on himalaya's "Manage accounts defined in the TOML configuration file.", and `account list` on "List all accounts declared in the configuration.", each keeping neverest's own paragraph after it.
- tests/account.rs `account_ls_lists_and_the_abouts_are_the_familys`.

## Capabilities moved

- [sync](../spec/sync.md): *The account group is the family's* names `ls` and gains the scenario *Listing with `ls`*.
