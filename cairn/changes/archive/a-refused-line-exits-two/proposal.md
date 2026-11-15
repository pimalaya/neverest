---
cairn: change
id: a-refused-line-exits-two
status: landed
created: 2026-11-13
---

# A refused line exits two

## Why

pimalaya-cli changed the family exit contract (its change a-refused-line-exits-two, 2026-11-13): 0 success, 1 failure, 2 a command line the grammar refuses (clap's own status), 3 and above a partial outcome. neverest used 2 for a run waiting for a person and 3 for one that could not do all its work, so a script could not tell a mistyped invocation from a parked conflict. No release carries the old numbers past 0.3.0, so this is a renumbering, not a migration beyond the CHANGELOG and MIGRATION.md.

## What

- src/main.rs parses through `pimalaya_cli::clap::parse::<Cli>()`: a refused line exits 2, and with a `--json` word before any `--` prints the failure object coded `usage-invalid` on stdout. neverest turns no other clap error into a failure (no REPL, no MCP), so `usage_error` has no other caller.
- `Exit::Conflicted` (a run waiting for a person) exits 3, `EXIT_PARTIAL`; `Exit::Incomplete` (a run, a drain, an init or an index that could not do all its work) exits 4. `check` with an endpoint down exits 3 through `PartialOutcome::default()`, unchanged in code.
- Doc comments, the `sync` long help, the tests, README, config.sample.toml, MIGRATION.md, CHANGELOG and the spec follow.
- tests/usage.rs: `neverest --json sync --no-such-flag` prints the `usage-invalid` object and exits 2; without `--json`, clap's message on stderr and exit 2.
