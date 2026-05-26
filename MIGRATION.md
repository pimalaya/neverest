# Migration guide

v0.1.0 and v1.0.0-beta were early releases sitting on top of `email-lib`; v1.0.0-rc is a rewrite on top of the I/O-free `io-*` ecosystem (`io-email`, `io-imap`, `io-jmap`, `io-m2dir`). This page lists the changes most likely to bite when upgrading; the full configuration schema lives in [config.sample.toml](./config.sample.toml).

## Highlights

- Tokio is gone; the binary is synchronous (`io-*` clients use `std::net`).
- **m2dir** replaces Maildir as the local sync target (https://man.sr.ht/~bitfehler/m2dir/), so flag round-trips are lossless. Maildir consumers should point at a fresh m2dir root and resync from IMAP/JMAP.
- **JMAP** is now a supported backend.
- **Notmuch** is removed.
- **Keyring** and **OAuth** are out of the binary: use [pimalaya/mimosa](https://github.com/pimalaya/mimosa) and [pimalaya/ortie](https://github.com/pimalaya/ortie) as command-sourced secrets.

## From v1.0.0-beta to v1.0.0-rc

Everything in the v0.1.0 → v1.0.0-rc section above applies. v1.0.0-beta only added a few cosmetic changes on top of v0.1.0; the deltas to watch are:

### CLI

| v1.0.0-beta | v1.0.0-rc |
|---|---|
| `doctor <account>` (aliases `check`, `check-up`, `checkup`) | `check -a <account>` (no other aliases) |
| `--debug` (alias for `RUST_LOG=debug`) | `--log-level debug` (alias `--log`) |
| `--trace` (alias for `RUST_LOG=trace` + backtrace) | `--log-level trace` |

### Configuration

| v1.0.0-beta | v1.0.0-rc |
|---|---|
| `folder.filters = "..."` (already plural in v1.0.0-beta) | `mailbox.filters = "..."` |
| `envelope.filters.{before,after}` | removed |
| `left\|right.folder.aliases.<name> = "..."` (per-side) | `[accounts.<account>.mailbox.alias]` `<name> = "..."` (single shared table) |

`color-eyre`'s spantrace/backtrace output is gone; errors now flow through `anyhow` + `pimalaya_cli::error::ErrorReport`. `tracing` is replaced by `log`.

## Suggested steps

1. Copy [config.sample.toml](./config.sample.toml) next to the old config.toml and port your accounts.
2. `neverest check -a <account>` to validate both sides.
3. `neverest init -a <account>` to write the initial cache.
4. `neverest sync -a <account> --dry-run` to inspect the first patch.
5. Drop `--dry-run`, replace the old config, done.

## From v0.1.0 to v1.0.0-rc

### CLI

| v0.1.0 | v1.0.0-rc |
|---|---|
| `synchronize <account>` | `sync -a <account>` |
| `check-up <account>` | `check -a <account>` |
| `configure <account>` | `configure -a <account>` |
| (none) | `init -a <account>` (mandatory once, before the first sync) |
| `-f` / `--include-folder` | `-m` / `--include-mailbox` |
| `-x` / `--exclude-folder` | `-x` / `--exclude-mailbox` |
| `-A` / `--all-folders` | `-A` / `--all-mailboxes` |
| `-o {plain,json}` | `--json` |
| `-C` / `--color` | removed (color follows the terminal) |
| `RUST_LOG=...` only | `--log-level` (alias `--log`), `--log-file <PATH>` |

The positional `<account>` argument becomes an optional `-a` / `--account <NAME>` flag, falling back to the entry marked `default = true`.

### Configuration

| v0.1.0 | v1.0.0-rc |
|---|---|
| `folder.filter = "..."` | `mailbox.filters = "..."` |
| `folder.filter.{include,exclude}` | `mailbox.filters.{include,exclude}` |
| `left.backend.type = "imap"` + `host`/`port`/`encryption`/`auth` | `left.imap.server = "..."` + `imap.tls.*` + `imap.sasl.*` |
| `left.backend.type = "maildir"` + `root-dir` | `left.m2dir.root` |
| `left.backend.type = "notmuch"` | removed |
| `left.folder.permissions.{create,delete}` | `left.<backend>.mailbox.{create,delete}` |
| `left.flag.permissions.update` | `left.<backend>.flag.update` |
| `left.message.permissions.{create,delete}` | `left.<backend>.message.{create,delete}` |
| keyring entries | `{ command = "pass show ..." }` (or any other secret manager) |
| `auth.type = "oauth2"` | SASL `oauthbearer` / `xoauth2` with a token from [pimalaya/ortie](https://github.com/pimalaya/ortie) |
| `envelope.filter.{before,after}` | removed |

`left.<backend>.pool-size` is new (defaults: IMAP 8, JMAP 4, m2dir 8). The sync cache moved to $XDG_CACHE_HOME/neverest/<account>/state.json (JSON); its presence is the single source of truth for "this account is initialized".
