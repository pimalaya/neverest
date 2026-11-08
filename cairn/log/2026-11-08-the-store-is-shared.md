---
cairn: log
change: the-store-is-shared
date: 2026-11-08
---

# The store is shared

Every account naming no `store.root` now syncs into one store, `<state base>/neverest/` itself, and `-a/--account` is a repeatable filter: without it `sync`, `check`, `drain` and `init` run over every account, store by store and in name order, under each store's one lock. pimdir is multi-account by design (STORAGE §9.2) and the Android app already keeps every account in one store.

## What landed

- Store-wide ids carry the account: a pimdir source id is `<account>/<source>` and a collection id `<account>/<source>/<name>`. pimdir wants both unique store-wide (STORAGE §4.3, §9.2), and `delete_capabilities` keys on the source alone, so two accounts holding the wizard's `imap` source would have written into one `imap/INBOX` and wiped each other's declarations. `AccountConfig::namespaced` (src/config.rs) hands the driver the account as the store sees it, every endpoint keyed by its source id, so the engine, the declaration, the drain, the intents and the wire seam need no account of their own; `SyncOutput::name_endpoints` and `SyncEvent::name_endpoints` give reports and events back the configured names. An account name holding `/` is refused.
- The sidecar (src/offline/state.rs) is layout 2: one entry per account (mode, accepted mode, looked-up roles, taskless calendars), an account initialized once it has one. A sidecar of layout 1 or 0, or a database with none, is refused as the new `store-layout` code (src/error.rs), never migrated. `driver::create_store` writes the sidecar with the database, and clears the in-flight send mark; a reset leaves the mark.
- `driver::refuse_former_stores`: the default root holding no pimdir.db but a subdirectory that does is refused as `store-layout`, naming each former store and the order (sync with the previous neverest until nothing is queued, move it out, `neverest init`), before anything is created. `driver::check_store` runs it, then opens a reader (no lock, no write) for the version refusal, then the sidecar; a refused store fails the command, exit 1. The `store-version` remedy no longer names `--reset --force`: it moves the store directory away.
- `sync` (src/cli/sync.rs) groups the accounts by store, takes every store's lock up front in path order (a held one past the 60 s wait reports its accounts `busy`, exit 3), readies each account (`driver::prepare`: the sidecar entry, the mode check, the declaration), drains the store once (`driver::drain_store`, rows shared out by the collection's account) and runs each account. Its result is `{accounts: [{account, busy, code, error, report}]}`, the exit the furthest account's, a failed or busy account counting as incomplete (3). `account-started` announces each account under `--json`.
- `sync --reset` needs exactly one account and starts it over inside the shared store: it drops the account's collections (`delete_collection`, the recount in its transaction), declares its sources empty, runs the collector and stamps a fresh sidecar entry. A queued or parked action on its collections refuses it, forced or not, naming each row, `neverest drain` and `himalaya pimdir queue cancel`; without `--force`, conflicts, unpushed changes, pending deletes and retained items refuse it too. Nothing is moved aside any more.
- The parked rows a sync reports are the account's own collections', and the retention sweep purges the account's retained items one at a time (`list_retained`, `purge`): `purge_retained_before` is store-wide, so one account's `"0"` would have emptied every account's trash.
- `drain`, `init`, `check`, `index` and `conflict` follow: `drain` declares every account, drains once a store and reports each its own rows; `init` adds an account to the store, creating it when absent, and leaves an initialized one as it is; `check` reports `orphans`, the accounts a store holds that no configured account sharing it names; `index` builds each store of the accounts it runs over once; `conflict show` and `conflict resolve` need exactly one account, `conflict list` names each row's account.
- `default = true` is read and ignored with one warning; the wizard writes none and its closing hint names `neverest init` then `neverest sync`.
- `sync --accept-mode` now lets the refused run through: `check_mode` ignored the flag, so the acknowledgement the refusal names was refused too.
- Docs: config.sample.toml, README.md, MIGRATION.md (the reinstall order for the shared store, the readers' `pimdir.root` and `pimdir.account`), the src/main.rs header, CHANGELOG.md.

## Decided differently from the brief

- The ids gained the account, which the brief did not ask for: without it the shared store corrupts as soon as two accounts hold a source of one name.
- Accounts run in name order, store by store, not in config order: pimalaya-config merges the documents through a sorted TOML table, so the file's order is gone before neverest reads it.
- A configuration that does not validate fails the command before anything runs, like an unknown name, rather than failing one account: the configuration is one document.
- `init` over an initialized account reports it initialized already and exits 0, where it failed, since `init` now runs over every account by default.

## Evidence

`cargo test --all-features` 522 passed (497 unit, tests/accounts.rs 7, drain 9, intents 8, check 1), 68 ignored (the server suites); `cargo clippy --all-features --all-targets` clean; `cargo check` with no default features and with `search` alone. tests/accounts.rs, run against the code of d669915 (extracted with `git archive`), fails all 7; the new unit tests do not compile there.

## Left

- The Stalwart, Radicale and live suites were moved to the new layout by reading, not run: they need servers.
- A drain run under `-a` applies every account's queue, the store's being one, and reports only the run's accounts' rows.

## Capabilities moved

- sync: the shared store, the account in every store-wide id, the per-account sidecar, the former-layout refusal, `-a` as a filter, the per-account reset and retention sweep, the orphan report.
