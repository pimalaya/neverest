---
cairn: tasks
change: the-wizard-generates-it-never-edits
---

- [x] `src/cli/configure.rs` holds the contract: bail off a terminal, read the existing names and default claim, derive a free name, claim the default only when nothing else does, render, append or save or print
- [x] `AccountConfig::render` renders one `[accounts.<name>]` table, groups ordered and each endpoint lifted to the top of its own
- [x] `discover::run` returns the proposed name beside the account and nothing else
- [x] `src/wizard/edit.rs` and `Config::write` deleted, `AccountConfig::direct_sources` with them
- [x] `offer_configuration` and `print_welcome` move beside the command
- [x] `configure` takes no account: the dispatcher stops handing it `-a`
- [x] `ConfigureOutput` registered in the schema registry
- [x] Tests: a taken name gets a suffix, an appended account keeps the existing one, a missing configuration constrains nothing, several sources still render as one table
- [x] Docs: README, MIGRATION, CHANGELOG, crate header
- [x] Fold the delta, log
