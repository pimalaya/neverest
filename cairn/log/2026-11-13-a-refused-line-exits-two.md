---
cairn: log
change: a-refused-line-exits-two
date: 2026-11-13
---

# A refused line exits two

neverest follows pimalaya-cli's exit contract (pimalaya-cli a-refused-line-exits-two, 2026-11-13): 2 is a refused command line, and the partial outcomes start at 3.

## What landed

- src/main.rs: `pimalaya_cli::clap::parse::<Cli>()` replaces `Cli::parse()`. A refused line exits 2; with `--json` before any `--` it prints `{"code":"usage-invalid","error":...,"sources":[],"backtrace":null}` on stdout. neverest has no REPL and no MCP server, and its only `try_parse_from` calls are in tests, so nothing else turns a clap error into a failure and `usage_error` has no caller here.
- src/cli/exit.rs: `Exit::Conflicted` exits `EXIT_PARTIAL` (3) and `Exit::Incomplete` 4, where they exited 2 and 3. `init`, `drain` and `index` share `Exit::Incomplete`, so a failed account, a busy store or a busy index exits 4 too. `check` returns `PartialOutcome::default()`, now 3.
- Doc comments follow (src/sync/report.rs, src/cli/{check,drain,index,init,sync}.rs), the `sync` long help naming 0, 1, 2, 3 and 4.
- Tests: src/cli/exit.rs asserts 3 and 4; every integration test passing an exit code moved (2 to 3, 3 to 4), the throttle rerun in tests/common/mod.rs waiting on 4. tests/usage.rs is new: `--json sync --no-such-flag` prints the `usage-invalid` object and exits 2, and without `--json` clap's message goes to stderr with stdout empty. The first fails against the old `Cli::parse()`, which printed nothing on stdout.
- README (Conflicts, accounts), config.sample.toml, MIGRATION.md (highlights, the v0.3.0 section) and CHANGELOG (a BREAKING entry under Changed; the Unreleased entries naming 2 and 3 renumbered in place).

## Evidence

See the wizard-removed log of the same day: fmt, clippy (default, `--all-features`, `--no-default-features`) and the tests that run without a server pass. The server suites (Stalwart, Radicale, live) were not run; their expected codes were renumbered by reading.

## Capabilities moved

- sync (cairn/spec/sync.md): added "A refused command line exits 2"; modified "A conflicted run succeeds, with its own exit code" (3) and "An incomplete run has its own exit code" (4), and every requirement naming an outcome's number: "`-a` filters the accounts a command runs over", "A check reports every endpoint", "A check opens the send channel too", "A divergence with no common ancestor parks rather than resolving itself", "A refused write names the item as a person knows it", "A send that may have been delivered is never sent again", "A listing missing most of a collection waits for a person", "A change over an unreadable body is withheld and reported", "Neverest is the store's sole owner and drains the queue first", "A drain applies the queue without syncing", "A run holds the store lock, waiting bounded", "A quota refusal keeps the send queued", "An unreached endpoint is named in the report", "A throttled provider is waited out, then named", "A push no server answered is pushed again", "The store's owner builds its search index".
