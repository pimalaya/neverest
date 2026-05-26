<div align="center">
  <img src="./logo.svg" alt="Logo" width="128" height="128" />
  <h1>📫 Neverest</h1>
  <p>CLI to synchronize, backup and restore emails</p>
  <p>
    <a href="https://matrix.to/#/#pimalaya:matrix.org"><img alt="Matrix" src="https://img.shields.io/badge/chat-%23pimalaya-blue?style=flat&logo=matrix&logoColor=white"/></a>
    <a href="https://fosstodon.org/@pimalaya"><img alt="Mastodon" src="https://img.shields.io/badge/news-%40pimalaya-blue?style=flat&logo=mastodon&logoColor=white"/></a>
  </p>
</div>

> [!CAUTION]
> Neverest is in active development and currently shipped as `v1.0.0-rc`. Expect breaking changes between releases until stabilization.

> [!IMPORTANT]
> This README documents Neverest v1, which is **not yet released**. If you are running v0.1, refer to the [v0.1.0 README](https://github.com/pimalaya/neverest/blob/v0.1.0/README.md) instead. The [MIGRATION.md](./MIGRATION.md) guide walks v1 users through the breaking changes.

## Table of contents

- [Features](#features)
- [Installation](#installation)
  - [Pre-built binary](#pre-built-binary)
  - [Cargo](#cargo)
  - [Nix](#nix)
  - [Sources](#sources)
- [Configuration](#configuration)
- [Usage](#usage)
  - [Initializing an account](#initializing-an-account)
  - [Running a sync](#running-a-sync)
  - [Mailbox filters and per-side permissions](#mailbox-filters-and-per-side-permissions)
  - [Migrating from Maildir](#migrating-from-maildir)
  - [Checking a configuration](#checking-a-configuration)
- [Social](#social)
- [Sponsoring](#sponsoring)

## Features

- Remote backend support: **IMAP**, **JMAP**
- Local (filesystem) backend support: **m2dir** <sup>[specs](https://man.sr.ht/~bitfehler/m2dir/)</sup>
- **Simple auth** support for IMAP: anonymous, login, plain, oauthbearer, xoauth2, scram-sha-256
- **HTTP auth** support for JMAP: basic, bearer, raw header
- **TLS** support:
  - [Rustls](https://crates.io/crates/rustls) with ring crypto
  - [Rustls](https://crates.io/crates/rustls) with aws crypto (requires `rustls-aws` feature)
  - [Native TLS](https://crates.io/crates/native-tls) (requires `native-tls` feature)
- **Discovery** support (wizard only):
  - PACC <sup>[specs](https://datatracker.ietf.org/doc/html/draft-ietf-mailmaint-pacc)</sup>
  - Autoconfiguration (Thunderbird) <sup>[specs](https://wiki.mozilla.org/Thunderbird:Autoconfiguration)</sup>
  - SRV DNS lookups <sup>[rfc6186](https://datatracker.ietf.org/doc/html/rfc6186)</sup>
- **Mailbox filters** (include / exclude / all), applied symmetrically to both sides
- **Per-side permissions** gating `create` / `delete` on mailboxes and messages, plus `update` on flags
- **Per-side connection pools** with one client per worker
- **Incremental cache** at `$XDG_CACHE_HOME/neverest/<account>/state.json`
- **Dry-run** mode (`-d`) prints the patch the sync would apply without touching either side
- **JSON** output via `--json`

> [!TIP]
> Neverest is written in [Rust](https://www.rust-lang.org/) and uses [cargo features](https://doc.rust-lang.org/cargo/reference/features.html) to gate backend support. The default feature set is declared in [Cargo.toml](./Cargo.toml).

## Installation

### Pre-built binary

Neverest is not yet released, therefore the only way to get a pre-built binary is to check out the [releases](https://github.com/pimalaya/neverest/actions/workflows/releases.yml) GitHub workflow and look for the *Artifacts* section.

> [!NOTE]
> Such binaries are built with the default cargo features. If you need specific features, please use another installation method.

### Cargo

```
cargo install --locked --git https://github.com/pimalaya/neverest.git
```

With only IMAP + m2dir support:

```
cargo install --locked --git https://github.com/pimalaya/neverest.git \
  --no-default-features \
  --features imap,m2dir,rustls-ring
```

### Nix

If you have the [Flakes](https://nixos.wiki/wiki/Flakes) feature enabled:

```
nix profile install github:pimalaya/neverest
```

Or run without installing:

```
nix run github:pimalaya/neverest
```

### Sources

```
git clone https://github.com/pimalaya/neverest
cd neverest
nix run
```

## Configuration

Run `neverest`. With no configuration file on disk the wizard asks for an account name and an email address, runs provider discovery (PACC, then Thunderbird Autoconfiguration, then RFC 6186 SRV), prompts for IMAP or JMAP credentials based on what discovery returned, asks for a local m2dir store root for the other side, then writes the result to disk.

A persistent configuration is loaded from the first valid path among:

- `$XDG_CONFIG_HOME/neverest/config.toml`
- `$HOME/.config/neverest/config.toml`
- `$HOME/.neverestrc`

Override the path with `-c <PATH>` or `NEVEREST_CONFIG=<PATH>`; multiple paths can be passed at once, separated by `:`. The first one is the base and the rest are deep-merged on top.

See [config.sample.toml](./config.sample.toml) for a documented template covering every supported field. An existing account can be re-prompted later with `neverest configure` (or `neverest configure -a <account>` to target a non-default account): the wizard reuses the current values as defaults instead of re-running discovery.

## Usage

### Initializing an account

Before the first sync each account must be initialized once:

```
neverest init [-a|--account <NAME>]
```

The account flag is optional: when omitted, the account marked `default = true` in the configuration is used.

This opens both sides (IMAP CAPABILITY / JMAP session GET / m2dir store creation) so credential and network errors surface up front, then writes an empty cache snapshot at `$XDG_CACHE_HOME/neverest/<account>/state.json`. The presence of that file is the single source of truth for "this account is initialized"; `sync` refuses to run when it is missing and `init` refuses to run when it is present.

### Running a sync

```
neverest sync [-a|--account <NAME>]
```

Sync walks every mailbox surviving the filter, diffs the two sides against the cached snapshot, applies the resulting hunks through per-side connection pools, then prints a report covering created / updated / deleted mailboxes, flags and messages. Pass `-d` / `--dry-run` to print the patch without applying it.

Pass `--reset` to drop the cached state before running. Without `--include-mailbox`, the entire snapshot plus every IMAP / JMAP state token is cleared; with `--include-mailbox`, only the listed mailboxes are wiped. The first post-reset sync rebuilds the snapshot via a full re-list, equivalent to first-sync semantics.

### Mailbox filters and per-side permissions

Mailbox filters declared in the configuration apply symmetrically to both sides. They can be overridden per invocation with `-m / --include-mailbox`, `-x / --exclude-mailbox`, or `-A / --all-mailboxes` (the three flags are mutually exclusive). Matching is ASCII case-insensitive: `INBOX` matches `inbox`, but non-ASCII characters (umlauts, Cyrillic, accents) must be spelled exactly as the server reports them.

Per-side permissions live under each side's backend table and gate what the sync engine is allowed to mutate on that side:

```toml
[accounts.example]
left.m2dir.root = "~/.Mail/example"
left.m2dir.mailbox.create = false
left.m2dir.mailbox.delete = false
left.m2dir.flag.update = true
left.m2dir.message.create = true
left.m2dir.message.delete = false

right.imap.server = "imap.example.com"
right.imap.message.delete = false
```

All five permissions default to `true`. Setting any of them to `false` makes the engine treat the side as read-only for that operation; planned hunks that would violate the policy are dropped from the patch and surfaced in the report.

### Migrating from Maildir

Neverest does not ship an in-tree Maildir converter: keyword storage is not standardized across Maildir consumers (info-section letters, `dovecot-keywords`, `X-Keywords` / `X-Label` headers, …), so any local migration would silently lose or mangle flags depending on which tool wrote the source tree.

The recommended path for users coming from mbsync, OfflineIMAP or a Dovecot Maildir layout is to point Neverest at a fresh m2dir root and resync from the authoritative IMAP/JMAP server. Flags re-converge cleanly and the new cache reflects the actual server state.

> [!TIP]
> You can also use [m2m](https://github.com/pimalaya/m2m) to convert your Maildir structure into a m2dir one, which is more adapted for synchronization. But since no standards exist for managing custom flags in Maildir, it is still recommended to resync from IMAP/JMAP.

### Checking a configuration

```
neverest check [-a|--account <NAME>]
```

Opens both sides and asks each one to list mailboxes. The operation itself is cheap; the value is in surfacing the credential, network or config errors that would otherwise only show up during a real sync.

## Social

- Chat on [Matrix](https://matrix.to/#/#pimalaya:matrix.org)
- News on [Mastodon](https://fosstodon.org/@pimalaya) or [RSS](https://fosstodon.org/@pimalaya.rss)
- Mail at [pimalaya.org@posteo.net](mailto:pimalaya.org@posteo.net)

## Sponsoring

[![nlnet](https://nlnet.nl/logo/banner-160x60.png)](https://nlnet.nl/)

Special thanks to the [NLnet foundation](https://nlnet.nl/) and the [European Commission](https://www.ngi.eu/) that have been financially supporting the project for years:

- 2022 → 2023: [NGI Assure](https://nlnet.nl/project/Himalaya/)
- 2023 → 2024: [NGI Zero Entrust](https://nlnet.nl/project/Pimalaya/)
- 2024 → 2026: [NGI Zero Core](https://nlnet.nl/project/Pimalaya-PIM/)
- *2027 in preparation…*

If you appreciate the project, feel free to donate using one of the following providers:

[![GitHub](https://img.shields.io/badge/-GitHub%20Sponsors-fafbfc?logo=GitHub%20Sponsors)](https://github.com/sponsors/soywod)
[![Ko-fi](https://img.shields.io/badge/-Ko--fi-ff5e5a?logo=Ko-fi&logoColor=ffffff)](https://ko-fi.com/soywod)
[![Buy Me a Coffee](https://img.shields.io/badge/-Buy%20Me%20a%20Coffee-ffdd00?logo=Buy%20Me%20A%20Coffee&logoColor=000000)](https://www.buymeacoffee.com/soywod)
[![Liberapay](https://img.shields.io/badge/-Liberapay-f6c915?logo=Liberapay&logoColor=222222)](https://liberapay.com/soywod)
[![thanks.dev](https://img.shields.io/badge/-thanks.dev-000000?logo=data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMjQuMDk3IiBoZWlnaHQ9IjE3LjU5NyIgY2xhc3M9InctMzYgbWwtMiBsZzpteC0wIHByaW50Om14LTAgcHJpbnQ6aW52ZXJ0IiB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciPjxwYXRoIGQ9Ik05Ljc4MyAxNy41OTdINy4zOThjLTEuMTY4IDAtMi4wOTItLjI5Ny0yLjc3My0uODktLjY4LS41OTMtMS4wMi0xLjQ2Mi0xLjAyLTIuNjA2di0xLjM0NmMwLTEuMDE4LS4yMjctMS43NS0uNjc4LTIuMTk1LS40NTItLjQ0Ni0xLjIzMi0uNjY5LTIuMzQtLjY2OUgwVjcuNzA1aC41ODdjMS4xMDggMCAxLjg4OC0uMjIyIDIuMzQtLjY2OC40NTEtLjQ0Ni42NzctMS4xNzcuNjc3LTIuMTk1VjMuNDk2YzAtMS4xNDQuMzQtMi4wMTMgMS4wMjEtMi42MDZDNS4zMDUuMjk3IDYuMjMgMCA3LjM5OCAwaDIuMzg1djEuOTg3aC0uOTg1Yy0uMzYxIDAtLjY4OC4wMjctLjk4LjA4MmExLjcxOSAxLjcxOSAwIDAgMC0uNzM2LjMwN2MtLjIwNS4xNTYtLjM1OC4zODQtLjQ2LjY4Mi0uMTAzLjI5OC0uMTU0LjY4Mi0uMTU0IDEuMTUxVjUuMjNjMCAuODY3LS4yNDkgMS41ODYtLjc0NSAyLjE1NS0uNDk3LjU2OS0xLjE1OCAxLjAwNC0xLjk4MyAxLjMwNXYuMjE3Yy44MjUuMyAxLjQ4Ni43MzYgMS45ODMgMS4zMDUuNDk2LjU3Ljc0NSAxLjI4Ny43NDUgMi4xNTR2MS4wMjFjMCAuNDcuMDUxLjg1NC4xNTMgMS4xNTIuMTAzLjI5OC4yNTYuNTI1LjQ2MS42ODIuMTkzLjE1Ny40MzcuMjYuNzMyLjMxMi4yOTUuMDUuNjIzLjA3Ni45ODQuMDc2aC45ODVabTE0LjMxNC03LjcwNmgtLjU4OGMtMS4xMDggMC0xLjg4OC4yMjMtMi4zNC42NjktLjQ1LjQ0NS0uNjc3IDEuMTc3LS42NzcgMi4xOTVWMTQuMWMwIDEuMTQ0LS4zNCAyLjAxMy0xLjAyIDIuNjA2LS42OC41OTMtMS42MDUuODktMi43NzQuODloLTIuMzg0di0xLjk4OGguOTg0Yy4zNjIgMCAuNjg4LS4wMjcuOTgtLjA4LjI5Mi0uMDU1LjUzOC0uMTU3LjczNy0uMzA4LjIwNC0uMTU3LjM1OC0uMzg0LjQ2LS42ODIuMTAzLS4yOTguMTU0LS42ODIuMTU0LTEuMTUydi0xLjAyYzAtLjg2OC4yNDgtMS41ODYuNzQ1LTIuMTU1LjQ5Ny0uNTcgMS4xNTgtMS4wMDQgMS45ODMtMS4zMDV2LS4yMTdjLS44MjUtLjMwMS0xLjQ4Ni0uNzM2LTEuOTgzLTEuMzA1LS40OTctLjU3LS43NDUtMS4yODgtLjc0NS0yLjE1NXYtMS4wMmMwLS40Ny0uMDUxLS44NTQtLjE1NC0xLjE1Mi0uMTAyLS4yOTgtLjI1Ni0uNTI2LS40Ni0uNjgyYTEuNzE5IDEuNzE5IDAgMCAwLS43MzctLjMwNyA1LjM5NSA1LjM5NSAwIDAgMC0uOTgtLjA4MmgtLjk4NFYwaDIuMzg0YzEuMTY5IDAgMi4wOTMuMjk3IDIuNzc0Ljg5LjY4LjU5MyAxLjAyIDEuNDYyIDEuMDIgMi42MDZ2MS4zNDZjMCAxLjAxOC4yMjYgMS43NS42NzggMi4xOTUuNDUxLjQ0NiAxLjIzMS42NjggMi4zNC42NjhoLjU4N3oiIGZpbGw9IiNmZmYiLz48L3N2Zz4=)](https://thanks.dev/soywod)
[![PayPal](https://img.shields.io/badge/-PayPal-0079c1?logo=PayPal&logoColor=ffffff)](https://www.paypal.com/paypalme/soywod)
