---
cairn: tasks
change: the-store-is-shared
---

# Tasks

- [x] `config`: `Config::select` (every account in name order, or those `-a` names; an unknown name fails naming the configured ones; a name holding `/` refused; `default` warned about once), `exactly_one`, `AccountConfig::namespaced`.
- [x] `cli/main`: `-a/--account` repeatable, every command handed the list.
- [x] `error`: `store-layout`.
- [x] `offline/state`: layout 2, one entry per account, the previous shape refused as `store-layout`.
- [x] `offline/driver`: the default root `<state base>/neverest/`, the former-layout refusal, the store preflight (a reader, no write), the `store-version` remedy (move the store away), the parked rows and the retention sweep scoped to the account, the drain lifted out of `run`.
- [x] `sync/report`, `sync/event`: the run's result over accounts, endpoints named as configured, `account-started`.
- [x] `cli/sync`: accounts in turn under one lock per store, busy after the bounded wait, the per-account reset.
- [x] `cli/drain`, `cli/init`, `cli/check` (orphans), `cli/index` (per store), `cli/conflict` (exactly one account to show or resolve, `account` on a listed row).
- [x] `cli/configure`: no `default`, the closing hint names `neverest sync`.
- [x] Tests, failing first on the old code: the default root, the former-layout refusal, the filter and its refusals, a reset dropping one account and leaving another, the sidecar per account, the orphan report, the per-account sweep; existing tests moved to the new layout.
- [x] Docs: config.sample.toml, README.md, MIGRATION.md, src/main.rs header.
- [x] Fold the delta into cairn/spec/sync.md, log, CHANGELOG, archive.
