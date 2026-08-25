# Migration guide

v0.1.0 and v1.0.0-beta were early releases sitting on top of `email-lib`. v1.0.0-rc is a full rewrite on top of the I/O-free `io-*` ecosystem, so the CLI, the configuration schema and the sync engine all changed shape. This page lists the changes most likely to bite when upgrading. The full configuration schema lives in [config.sample.toml](./config.sample.toml), and the exhaustive list of changes in [CHANGELOG.md](./CHANGELOG.md).

## Highlights

- The binary is synchronous: Tokio is gone, the `io-*` clients using `std::net`.
- **`left` and `right` are gone.** An account holds named **sources** over one pimdir store. A backend written directly under the account (`imap.server = "…"`) is sugar for a source named after its protocol, which is the whole configuration for a single-provider account; `sources.<name>.<protocol>.*` is the explicit form, and the only one able to express two sources of one protocol. A configuration still carrying `left` or `right` is refused, naming its replacement.
- **An account may hold several kinds.** Mail, contacts and calendar sources sit under one account and one store, their collections keyed apart so they never meet.
- **Mirroring is opt-in.** Two sources of one kind sharing a `collection.namespace` bind the same hub collections, and that sharing is what makes a change on one reach the other. Left on their default namespaces (their own names) they cache side by side and never push to one another. The old `left` / `right` pair was always the first, so write the namespace on both to keep it.
- **`store.retention` and `store.hydration` are gone.** What the store keeps is derived per kind and reported by every run and by `neverest check`: one source keeps every body, two sources sharing a namespace on an IMAP to IMAP pairing keep none and stream each crossing, anything else keeps what crossed. A configuration still carrying either key is refused.
- A source is a remote. **Local file backends are no longer sync sources**: the local pimdir store is the local replica, so a Maildir or m2dir tree beside it would be a second local copy. Resync from the authoritative server instead.
- **Microsoft Graph** and **CardDAV** are new backends. JMAP and Gmail sources parse but cannot be opened yet.
- **Notmuch** is removed.
- **Keyring** and **OAuth** are out of the binary: source secrets from a command instead, such as `pass` or `secret-tool`, and [ortie](https://github.com/pimalaya/ortie) for OAuth access tokens.
- The sync vocabulary is kind-neutral: **collections** and **items** rather than mailboxes and messages. The old spellings keep working as aliases for one release, except in the `--json` report.

## Suggested steps

1. Copy [config.sample.toml](./config.sample.toml) next to the old config.toml and port your accounts.
2. Run `neverest check -a <account>` to validate every source and read back what the store will keep for each namespace.
3. Run `neverest init -a <account>` to create the store.
4. Run `neverest sync -a <account> --dry-run` to inspect the first patch.
5. Drop `--dry-run`, replace the old config, done.

## From v1.0.0-beta to v1.0.0-rc

Everything in the v0.1.0 section below applies too: v1.0.0-beta only added a few cosmetic changes on top of v0.1.0.

### CLI

| v1.0.0-beta | v1.0.0-rc |
|---|---|
| `doctor <account>` (aliases `check`, `check-up`, `checkup`) | `check -a <account>` |
| `--debug` (alias for `RUST_LOG=debug`) | `--log-level debug` (alias `--log`) |
| `--trace` (alias for `RUST_LOG=trace` plus a backtrace) | `--log-level trace` |

### Configuration

| v1.0.0-beta | v1.0.0-rc |
|---|---|
| `folder.filters = "..."` | `collection.filter = "..."` |
| `envelope.filters.{before,after}` | removed |
| `left\|right.folder.aliases.<name>` | removed |

`color-eyre`'s spantrace and backtrace output is gone: errors flow through `anyhow` and pimalaya-cli's error report. `tracing` is replaced by `log`.

## From v0.1.0 to v1.0.0-rc

### CLI

| v0.1.0 | v1.0.0-rc |
|---|---|
| `synchronize <account>` | `sync -a <account>` |
| `check-up <account>` | `check -a <account>` |
| `configure <account>` | `configure -a <account>` |
| (none) | `init -a <account>`, mandatory once, before the first sync |
| `-f` / `--include-folder` | `-m` / `--include-collection` |
| `-x` / `--exclude-folder` | `-x` / `--exclude-collection` |
| `-A` / `--all-folders` | `-A` / `--all-collections` |
| `-o {plain,json}` | `--json` |
| `-C` / `--color` | removed, color follows the terminal |
| `RUST_LOG=...` only | `--log-level` (alias `--log`), `--log-file <PATH>` |

The positional `<account>` argument becomes an optional `-a` / `--account <NAME>` flag, falling back to the entry marked `default = true`.

### Configuration

| v0.1.0 | v1.0.0-rc |
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
| `left` / `right` | `sources.<name>.<protocol>.*`, plus the same `collection.namespace` on both to keep them mirroring |
| account-level `collection.filter` | `<protocol>.collection.filter`, on the source it filters |
| `store.retention`, `store.hydration` | removed; derived per kind and reported |
| `smtp.*` at the account root | `smtp.*` again, or `sources.<name>.smtp.*`; at most one source per account may declare it |
| keyring entries | `{ command = ["pass", "show", "..."] }`, or any other secret manager |
| `auth.type = "oauth2"` | SASL `oauthbearer` or `xoauth2`, the token coming from [ortie](https://github.com/pimalaya/ortie) |
| `envelope.filter.{before,after}` | removed |

The sync cache is now the pimdir store at $XDG_STATE_HOME/neverest/&lt;account&gt;/, overridable with `store.root`. The presence of its database is the single source of truth for "this account is initialized". New account-level settings are `store.purge-after` (the retention sweep) and `connections`; per source, `<protocol>.item.update` gates in-place body edits, `<protocol>.pool-size` overrides the connection pool, and `<protocol>.collection.namespace` decides which sources meet.

A store written before collection ids carried their namespace is not read. Neverest refuses it and names `neverest sync --reset`, which drops the replica and resyncs: the store is a derived cache, so that costs a resync and loses only un-pushed local mutation.
