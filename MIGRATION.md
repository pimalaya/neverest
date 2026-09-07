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
- **`retain = true` alongside a target makes the store a backup** rather than a cache, so `sync --reset` destroys data rather than a derived copy.
- A source is a remote. **Local file backends are no longer sync sources**: the local pimdir store is the local replica, so a Maildir or m2dir tree beside it would be a second local copy. Resync from the authoritative server instead.
- **CardDAV** and **CalDAV** are new backends, behind the `dav` cargo feature and in the default set. **Microsoft Graph** is behind `msgraph`, which is not a default while it syncs mail alone. JMAP and Gmail sources parse but cannot be opened yet.
- **Notmuch** is removed.
- **Keyring** and **OAuth** are out of the binary: source secrets from a command instead, such as `pass` or `secret-tool`, and [ortie](https://github.com/pimalaya/ortie) for OAuth access tokens.
- The sync vocabulary is kind-neutral: **collections** and **items** rather than mailboxes and messages. The `mailbox`, `message` and `filters` spellings keep working as config aliases for one release; the v0.1 `folder` ones do not, and no alias survives in the `--json` report.
- **A sync can exit 2.** A run that reconciled every collection and still left something waiting for a person (a parked conflict, a refused duplicate `UID`, a rejected write) exits 2 rather than 0 or 1. A script treating any non-zero code as failure needs to tell 2 from 1.

## Suggested steps

1. Copy [config.sample.toml](./config.sample.toml) next to the old config.toml and port your accounts.
2. Run `neverest check -a <account>` to validate every endpoint and read back what the account does, in words.
3. Run `neverest init -a <account>` to create the store.
4. Run `neverest sync -a <account> --dry-run` to inspect the first patch.
5. Drop `--dry-run`, replace the old config, done.

## From a git master build

No v1.0.0 was ever released: what was prepared under that number ships as v0.2.0.

If you ran a build from master, the store format moved underneath it and is refused as stale. There is no migration: run `neverest sync --reset -a <account>` to drop it and resync.

With `retain = true` beside a target the store is a backup rather than a cache, so that reset destroys data.

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
