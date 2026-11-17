---
cairn: log
change: one-prelude-for-the-store-commands
date: 2026-11-17
---

# One prelude for the store commands

Finding NEV-14 of the CLI family analysis of 2026-11-15, its behaviour part and part of its structure.

## What landed

- `sync`, `sync --queue-only` and `sync --index-only` group their accounts by store through one `driver::group_stores` (validation, store directory, 0.x layout refusal) and word an uninitialized account through one `driver::not_initialized`, where each copied the block.
- `sync --index-only` over a store `init` never created now fails its accounts under `accounts` and exits 4, as the other two do, where it failed the command with exit 1.
- prof.rs and `NEVEREST_PROFILE`, named by the finding, were already gone.

## Left

- `run_pair` still takes 20 parameters and 15 functions still allow `clippy::too_many_arguments`: a context struct for the pair driver is a refactor of its own, with no behaviour to fix.

## Spec

sync.md, the `sync --index-only` requirement.
