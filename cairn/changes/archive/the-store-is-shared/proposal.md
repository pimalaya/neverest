---
cairn: change
id: the-store-is-shared
status: landed
created: 2026-11-08
---

# The store is shared

## Why

Each account gets its own store today, at `<state base>/neverest/<account>/` (cairn/spec/sync.md, "The default store root resolves on every supported platform"), and `-a/--account` picks one account, or the one claiming `default`. pimdir is multi-account by design: collections are grouped by account (STORAGE §9.2), bodies are content-addressed and deduped store-wide, a search hit is `(account, seq)`, and the Android app keeps every account in one store. A frontend reading two accounts reads two stores, and a body two accounts hold is stored twice.

## What

- **One store for every account.** An account naming no `store.root` uses `<state base>/neverest/` itself; `store.root` stays a per-account override, and accounts naming one root share it. Every collection stays grouped under its account, so a reader reads one account through its `pimdir.account` key.
- **Ids carry the account.** pimdir wants collection ids and source ids unique store-wide (STORAGE §4.3, §9.2): a source id is `<account>/<source>` and a collection id `<account>/<source>/<name>`. Two accounts each holding the wizard's `imap` source would otherwise write into one `imap/INBOX` collection, and each declaration would wipe the other's capabilities (`delete_capabilities` keys on the source alone). An account name holding `/` is refused. Reports and events keep naming an endpoint as the configuration does.
- **The former layout is refused once.** A `neverest/` directory holding no pimdir.db but a former per-account store is refused as `store-layout`, naming each such store and the order to follow: sync it with the neverest that wrote it until nothing is queued, move it out, run again.
- **The sidecar holds each account.** neverest.json records the layout once and the mode, the looked-up roles and the taskless calendars per account; a sidecar of the previous shape, or none beside a database, is refused as `store-layout`, never migrated.
- **The lock stays store-wide.** A run holds `sync.lock` for its whole length, whatever accounts it covers; a second run waits the bounded 60 s, then reports the store busy and exits 3. Accounts needing their own schedules give themselves their own `store.root`.
- **`-a` is a filter.** Repeatable; without it `sync`, `check`, `drain` and `init` run over every account, in name order, under one lock; with it over the named ones, an unknown name failing before anything runs. Each account's failure is its own, folded into one result listing every account's outcome, and the command exits 3 when one failed. `index` builds the stores of the accounts the filter keeps, each store once and whole. `conflict show`, `conflict resolve` and `sync --reset` need exactly one account. `default = true` is accepted and ignored with one warning; the wizard stops writing it.
- **A reset drops one account.** `sync --reset -a <account>` drops that account's collections, with their checkpoints and capability rows and its sidecar entry, then syncs it again; other accounts are untouched. It refuses while a collection of the account holds a queued or parked action, naming them and the way out, and without `--force` while it holds other work no server has.
- **The retention sweep is the account's.** `store.purge-after` purges the account's retained items only: a store-wide sweep would apply one account's delay to every other's.
- **`check` names orphans.** An account the store holds that no configured account names (a renamed TOML key) is reported, never deleted.

## Out of scope

- No migration of a former store: pimdir stores are reinstalled, never migrated (STORAGE §6), and pre-1.0 neverest keeps no migration code.
- No per-account lock: pimdir allows one owner process per store (STORAGE §8).
- Config order: pimalaya-config merges the documents through a sorted TOML table, so the order accounts run in is their name order.
