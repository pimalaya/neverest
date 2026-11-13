# Migration guide

v0.1.0 and v1.0.0-beta were early releases sitting on top of `email-lib`. v0.2.0 is a full rewrite on top of the I/O-free `io-*` ecosystem, so the CLI, the configuration schema and the sync engine all changed shape.

This page lists the changes most likely to bite when upgrading. The full configuration schema lives in [config.sample.toml](./config.sample.toml), and the exhaustive list of changes in [CHANGELOG.md](./CHANGELOG.md).

## Highlights

- The binary is synchronous: Tokio is gone, the `io-*` clients using `std::net`.
- **`left` and `right` are gone.** An account names its endpoints and the direction between them: **sources** over one pimdir store, optionally **targets** to copy them to. A configuration still carrying either is refused, naming its replacement.
- **A backend written directly under the account** (`imap.server = "…"`) is sugar for a source named after its protocol, which is the whole configuration for a single-provider account. `sources.<name>.<protocol>.*` is the explicit form, and the only one able to express two sources of one protocol.
- **An account may hold several kinds.** Mail, contacts and calendar sources sit under one account and one store, their collections keyed apart so they never meet.
- **What an account does is its arity plus two flags**, and `collection.namespace` is gone. With no `targets` every source syncs into the local store, isolated from the others; with them the source is copied to each target.
- **`one-way = true` makes the sources authoritative**, so the other side is overwritten rather than merged and nothing is reported as a conflict. The old `left` / `right` pair was a two-way mirror, which is one source and one target with `one-way` left off.
- **`store.retention` and `store.hydration` are gone.** Whether the store keeps bodies is the account's `retain`: true with no target, since the store is the destination, false with targets, a configuration naming both having asked to copy between them. A configuration still carrying either key is refused.
- **`retain = true` alongside a target makes the store a backup** rather than a cache, so `sync --reset` refuses to drop it while it holds anything no server has, and `sync --reset --force` moves it aside.
- A source is a remote. **Local file backends are no longer sync sources**: the local pimdir store is the local replica, so a Maildir or m2dir tree beside it would be a second local copy. Resync from the authoritative server instead.
- **CardDAV** and **CalDAV** are new backends, behind the `dav` cargo feature and in the default set. **Microsoft Graph** is behind `msgraph`, which is not a default while it syncs mail alone. JMAP and Gmail sources parse but cannot be opened yet.
- **Notmuch** is removed.
- **Keyring** and **OAuth** are out of the binary: source secrets from a command instead, such as `pass` or `secret-tool`, and [ortie](https://github.com/pimalaya/ortie) for OAuth access tokens.
- The sync vocabulary is kind-neutral: **collections** and **items** rather than mailboxes and messages. The `mailbox`, `message` and `filters` spellings keep working as config aliases for one release; the v0.1 `folder` ones do not, and no alias survives in the `--json` report.
- **A sync can exit 3 or 4.** A run that reconciled every collection and still left something waiting for a person (a parked conflict, a refused duplicate `UID`, a rejected write) exits 3 rather than 0 or 1, and one that could not do all its work exits 4. A script treating any non-zero code as failure needs to tell 3 and 4 from 1. Exit 2 is a command line neverest refused.

## Suggested steps

1. Copy [config.sample.toml](./config.sample.toml) next to the old config.toml and port your accounts.
2. Run `neverest check` to validate every endpoint and read back what each account does, in words.
3. Run `neverest init` to create the store every account shares.
4. Run `neverest sync --dry-run` to inspect the first patch.
5. Drop `--dry-run`, replace the old config, done.

## From v0.3.0, or any earlier build, to the next release

Two things change at once, and neither is migrated:

- The store follows pimdir storage-draft-10, sync-draft-10 and search-draft-10 (io-pimdir 0.11). A store any earlier neverest wrote, v0.3.0 or a git build, is refused by every profile as `store-version`.
- **Every account shares one store**: `neverest/` under the state directory itself (`$XDG_STATE_HOME/neverest` on Linux, `~/Library/Application Support/neverest` on macOS, `%LOCALAPPDATA%\neverest` on Windows), no longer one directory per account under it. A neverest directory still holding the per-account stores an earlier neverest kept is refused as `store-layout`, naming each, and so is a `store.root` store whose neverest.json an earlier neverest wrote, or kept beside its pimdir.db.

What only an old store holds is the work it queued (a send, an edit or a create staged by a frontend) and its retained items, so reinstall in this order:

1. With the neverest that wrote the stores, run `neverest sync -a <account>` for each account until nothing is queued: the run exits 0, its report lists no `parked` action, no `submitted` entry left parked and no conflict, and `neverest drain -a <account>` applies nothing more. Settle or cancel what stays parked.
2. Upgrade neverest.
3. Move every old store out of the neverest directory, deleting nothing: `mv $XDG_STATE_HOME/neverest $XDG_STATE_HOME/neverest-v0.3` on Linux moves them all at once. Move a `store.root` directory away too, or point `store.root` at a new path.
4. Run `neverest init`, which adds every account to the new shared store, then `neverest sync`, which lists every collection again and downloads its bodies.

Keep the moved stores until the new one holds what you expect: their retained items are only there.

A store another user can read is refused too, as `store-exposed`: one `neverest init` creates is its user's alone, and an older one moved back in place needs `chmod 700` on its directory and `chmod 600` on its pimdir.db.

What else changes:

- **`-a/--account` is a filter**, and repeatable. Without it, `sync`, `check`, `drain` and `init` run over every account, one after the other; `index` builds every store the accounts use. `conflict show`, `conflict resolve` and `sync --reset` act on one account, so `-a` names it when there are several. `default = true` is read and ignored, with a warning; drop it at your convenience.
- **One run, one lock.** The store is locked for the whole run, so two accounts of one store never sync at once, and a second run waits up to a minute then reports the store busy (exit 4). An account that needs its own schedule (a cron of its own, a frontend-triggered run that must not wait) names its own `store.root`.
- **An account's failure is its own.** A run goes on with the next account and exits 4 when one failed. Under `--json`, `sync`, `drain`, `init` and `check` print one result listing every account (`{"accounts": [...]}`, a sync's report under `accounts[].report`), and `index` one listing every store (`{"stores": [...]}`); `sync` announces each account with an `account-started` event.
- **Store ids carry the account.** A source id is `<account>/<source>` and a collection id `<account>/<source>/<name>` (`work/imap/INBOX`, where it was `imap/INBOX`), so two accounts with an `imap` source never share a collection. A report still names an endpoint as the configuration does (`imap`). An account name holding `/` is refused.
- **`sync --reset` starts one account over**: it drops that account's collections and syncs them again, every other account untouched. `--force` drops the conflicts, unpushed edits and retained items they hold rather than moving a store aside; a queued or parked action still refuses the reset until a drain applies it or its frontend cancels it.
- **Frontends** (himalaya, calendula, cardamum): point `pimdir.root` at the neverest directory and name the account with `pimdir.account`. A collection named by its id gains the account (`himalaya -f work/imap/INBOX`); a role (`inbox`, `sent`) keeps working.
- **neverest keeps its own files under the store's local/**: neverest.json, sync.lock and the mark of a send being handed over, where they stood beside pimdir.db. A script reading or locking one names it there (`local/sync.lock`).
- **A send that may have been delivered is `unconfirmed`**: its report entry carries `unconfirmed: true` and an `error` beginning `unconfirmed: `, the state every pimdir frontend reads, where a git build wrote `uncertain`.
- `neverest check` names, under `orphans`, an account the store holds that no configured account names: renaming an account's TOML key leaves its collections under the old name until the key is renamed back.
- **Exit statuses follow the Pimalaya exit contract.** A sync that waits for a person exits 3, where v0.3.0 exited 2, and one that could not do all its work 4, where it exited 3; `check` with an endpoint down exits 3. Exit 2 is now a command line neverest refused, printed under `--json` as a failure object coded `usage-invalid`.
- **The configuration wizard is gone**: `neverest configure`, the offer a bare `neverest` made without a configuration, and the `wizard` cargo feature. Write the configuration by hand from [config.sample.toml](./config.sample.toml); a command finding none fails naming the path it looked for.

## From a git master build

No v1.0.0 was ever released: what was prepared under that number ships as v0.2.0.

If you ran a build from master, its store is refused as `store-version`: reinstall it as the section above says.

## From v1.0.0-beta to v0.2.0

Everything in the v0.1.0 section below applies too: v1.0.0-beta only added a few cosmetic changes on top of v0.1.0.

### CLI

| v1.0.0-beta | v0.2.0 |
|---|---|
| `doctor <account>` (aliases `check`, `check-up`, `checkup`) | `check -a <account>` |
| `--debug` (alias for `RUST_LOG=debug`) | `--log-level debug` (alias `--log`) |
| `--trace` (alias for `RUST_LOG=trace` plus a backtrace) | `--log-level trace` |

### Configuration

| v1.0.0-beta | v0.2.0 |
|---|---|
| `folder.filters = "..."` | `collection.filter = "..."` |
| `envelope.filters.{before,after}` | removed |
| `left\|right.folder.aliases.<name>` | removed |

`color-eyre`'s spantrace and backtrace output is gone: errors flow through `anyhow` and pimalaya-cli's error report. `tracing` is replaced by `log`.

## From v0.1.0 to v0.2.0

### CLI

| v0.1.0 | v0.2.0 |
|---|---|
| `synchronize <account>` | `sync -a <account>` |
| `check-up <account>` | `check -a <account>` |
| `configure <account>` | `configure`, which generates a new account rather than editing one |
| (none) | `init -a <account>`, mandatory once, before the first sync |
| `-f` / `--include-folder` | `-m` / `--include-collection` |
| `-x` / `--exclude-folder` | `-x` / `--exclude-collection` |
| `-A` / `--all-folders` | `-A` / `--all-collections` |
| `-o {plain,json}` | `--json` |
| `-C` / `--color` | removed, color follows the terminal |
| `RUST_LOG=...` only | `--log-level` (alias `--log`), `--log-file <PATH>` |

The positional `<account>` argument becomes an optional `-a` / `--account <NAME>` flag, falling back to the entry marked `default = true`.

### Configuration

| v0.1.0 | v0.2.0 |
|---|---|
| `folder.filter = "..."` | `collection.filter = "..."` |
| `folder.filter.{include,exclude}` | `collection.filter.{include,exclude}` |
| `folder.aliases` | removed |
| `left.backend.type = "imap"` plus `host` / `port` / `encryption` / `auth` | `imap.server = "..."` plus `imap.tls.*` and `imap.sasl.*` |
| `left.backend.type = "maildir"` plus `root-dir` | removed |
| `left.backend.type = "notmuch"` | removed |
| `left.folder.permissions.{create,delete}` | `<protocol>.collection.{create,delete}` |
| `left.flag.permissions.update` | `<protocol>.flag.update` |
| `left.message.permissions.{create,delete}` | `<protocol>.item.{create,delete}` |
| `left` / `right` | `sources.<name>.<protocol>.*` and `targets.<name>.<protocol>.*`, plus `one-way = true` to copy rather than merge |
| account-level `collection.filter` | `<protocol>.collection.filter`, on the source it filters |
| `store.retention`, `store.hydration` | removed; the account's `retain` |
| `smtp.*` at the account root | `smtp.*` again (`smtp.server` plus `smtp.tls.*` and `smtp.sasl.*`, spelled exactly as the IMAP ones), or `sources.<name>.smtp.*`; at most one source per account may declare it |
| keyring entries | `{ command = ["pass", "show", "..."] }`, or any other secret manager |
| `auth.type = "oauth2"` | SASL `oauthbearer` or `xoauth2`, the token coming from [ortie](https://github.com/pimalaya/ortie) |
| `envelope.filter.{before,after}` | removed |

The sync cache is now the pimdir store at `neverest/<account>/` under the platform's state location: `$XDG_STATE_HOME` on Linux and the BSDs, `~/Library/Application Support` on macOS, `%LOCALAPPDATA%` on Windows. Override it with `store.root`. The presence of its database is the single source of truth for "this account is initialized".

New account-level settings are `one-way` and `retain` (what the account does), `store.purge-after` (the retention sweep) and `connections`. Per source, `<protocol>.item.update` gates in-place body edits and `<protocol>.pool-size` overrides the connection pool.

Nothing of the v0.1 cache is read: `init` builds the store from scratch and the first sync fills it.

Neverest stamps the account's mode beside the store and compares it every run. Turning `one-way` on over an account that synced both ways is refused once, the run that follows being the one that discards what the previous mode was merging.

`neverest sync --accept-mode` says you meant it and is remembered. A `retain` that drops from true to false, and a change in the number of endpoints, are reported and do not block.
