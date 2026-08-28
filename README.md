<div align="center">
  <img src="./logo.svg" alt="Logo" width="128" height="128" />
  <h1>📫 Neverest</h1>
  <p>CLI to synchronize PIM collections: mail, contact, calendar…</p>
  <p>
    <a href="https://matrix.to/#/#pimalaya:matrix.org"><img alt="Matrix" src="https://img.shields.io/badge/chat-%23pimalaya-blue?style=flat&logo=matrix&logoColor=white"/></a>
    <a href="https://fosstodon.org/@pimalaya"><img alt="Mastodon" src="https://img.shields.io/badge/news-%40pimalaya-blue?style=flat&logo=mastodon&logoColor=white"/></a>
    <a href="https://pimalaya.org/sponsor/"><img alt="Sponsor" src="https://img.shields.io/badge/sponsor-pink?style=flat&logo=github-sponsors&logoColor=white"/></a>
  </p>
</div>

> [!CAUTION]
> Neverest is in active development and currently shipped as `v1.0.0-rc`. Expect breaking changes between releases until stabilization.

> [!IMPORTANT]
> This README documents Neverest v1.0.0-rc, which is not released yet. Two releases exist: refer to the [v1.0.0-beta README](https://github.com/pimalaya/neverest/blob/v1.0.0-beta/README.md) or the [v0.1.0 README](https://github.com/pimalaya/neverest/blob/v0.1.0/README.md) for the one you are running, and to [MIGRATION.md](./MIGRATION.md) for the upgrade path from either.

## Table of contents

- [Features](#features)
- [Installation](#installation)
- [Configuration](#configuration)
- [Usage](#usage)
- [AI policy](https://github.com/pimalaya/.github/blob/master/AI_POLICY.md)
- [License](#license)
- [Social](#social)
- [Contributing](./CONTRIBUTING.md)
- [Sponsoring](#sponsoring)

## Features

- **PIM domain** support: **mail** via IMAP and Microsoft Graph (JMAP and Gmail configure but have no backend yet), **contacts** via CardDAV <sup>[rfc6352](https://www.iana.org/go/rfc6352)</sup> and **calendar** via CalDAV <sup>[rfc4791](https://www.iana.org/go/rfc4791)</sup> (both require the `dav` feature); one account syncs several at once
- **Local pimdir store** <sup>[specs](https://github.com/pimalaya/pimdir)</sup>: the single local copy an app reads, holding every domain an account syncs
- **Retention**: a removed item is kept, never lost, and reclaimed on a schedule
- **Relay** mode: a body crossing two IMAP servers is streamed server-to-server, never stored
- **Queued submission**: a message a frontend enqueued leaves through its source's send channel
- **Auth** support: anonymous, login, plain, oauthbearer, xoauth2, scram-sha-256 for IMAP; basic and bearer for CardDAV and CalDAV; OAuth 2.0 bearer tokens for Microsoft Graph
- **TLS** support: [Rustls](https://crates.io/crates/rustls) with ring or aws crypto (`rustls-aws` feature), [Native TLS](https://crates.io/crates/native-tls) (`native-tls` feature)
- **Discovery** support: known provider rules, PACC <sup>[specs](https://datatracker.ietf.org/doc/html/draft-ietf-mailmaint-pacc)</sup>, Autoconfiguration <sup>[specs](https://wiki.mozilla.org/Thunderbird:Autoconfiguration)</sup>, SRV <sup>[rfc6186](https://datatracker.ietf.org/doc/html/rfc6186)</sup>, DAV <sup>[rfc6764](https://datatracker.ietf.org/doc/html/rfc6764)</sup>
- **Interactive wizard** turning an email address into a tested account
- **TOML configuration** with multi-account support, and **JSON** output via `--json`

> [!TIP]
> Neverest is written in [Rust](https://www.rust-lang.org/) and uses [cargo features](https://doc.rust-lang.org/cargo/reference/features.html) to gate backend support. The default feature set is declared in [Cargo.toml](./Cargo.toml).

## Installation

### Pre-built binary

Neverest can be installed with the installer:

*As root:*

```sh
curl -sSL https://raw.githubusercontent.com/pimalaya/neverest/master/install.sh | sudo sh
```

*As a regular user:*

```sh
curl -sSL https://raw.githubusercontent.com/pimalaya/neverest/master/install.sh | PREFIX=~/.local sh
```

Neverest v1 is not released yet, so the installer has nothing to fetch. Until then, check out the [releases](https://github.com/pimalaya/neverest/actions/workflows/releases.yml) GitHub workflow and look for the *Artifacts* section.

> [!NOTE]
> Such binaries are built with the default cargo features. If you need specific features, please use another installation method.

### Cargo

```sh
cargo install --locked --git https://github.com/pimalaya/neverest.git
```

With only IMAP support:

```sh
cargo install --locked --git https://github.com/pimalaya/neverest.git \
  --no-default-features \
  --features imap,smtp,rustls-ring
```

### Nix

If you have the [Flakes](https://nixos.wiki/wiki/Flakes) feature enabled:

```sh
nix profile install github:pimalaya/neverest
```

Or run without installing:

```sh
nix run github:pimalaya/neverest
```

### Sources

```sh
git clone https://github.com/pimalaya/neverest
cd neverest
nix run
```

## Configuration

A configuration is loaded from the first valid path among:

- `$XDG_CONFIG_HOME/neverest/config.toml`
- `$HOME/.config/neverest/config.toml`
- `$HOME/.neverestrc`

Override the path with `-c <PATH>` or `NEVEREST_CONFIG=<PATH>`; multiple paths can be passed at once, separated by `:`. The first one is the base and the rest are deep-merged on top. The full field reference lives in [config.sample.toml](./config.sample.toml).

Run `neverest` with no configuration file on disk and a minimal wizard asks for an email address, searches the services reachable from it, prompts the authentication the chosen one advertises, tests the connection, then offers to write the result. It sets up **one account with one backend**, the offline replica most setups want, and nothing more: a second kind, a mirror between two providers, a fan-in are all written by hand against [config.sample.toml](./config.sample.toml). `neverest configure` runs the same flow again over an existing account. Declining the save prints the configuration on stdout, and a redirected stdout skips the prompts altogether, so `neverest > config.toml` writes the file itself.

An account is one pimdir store fed by one or more named **sources**, each a remote, and it may hold several kinds at once. What it does is its arity plus two flags, so there is no mode to name: with no `targets` every source syncs into the local store, which is the offline replica; with them the source is copied to each target. `one-way` makes the sources authoritative, so the other side's changes are overwritten rather than merged, and `retain` says whether the store keeps bodies or is only the ledger it has to be in every mode. Sources never meet, so several of them cache side by side for a frontend to union at display time.

## Usage

Every command carries its own `--help`, the source of truth for its flags and syntax.

```sh
neverest init  -a <account>
neverest sync  -a <account> --dry-run
neverest sync  -a <account> --include-collection INBOX
neverest check -a <account>
```

An account is initialized once, which opens every source so credential and network errors surface up front, then creates the empty store. `sync` refuses to run without it, and `init` refuses to run over it. `--reset` drops the cached state before a run, rebuilding it as a first sync would.

### Retention and backup

The store never truly deletes an item. When its last binding vanishes the row is retained: hidden from the sync and from listings, but kept with its body. Reclaiming is explicit and time-based, and neverest is the sweeper: after each sync it purges every retained item older than `store.purge-after`, then reports how many items and bytes it freed. Leaving the delay unset means never purge, `"0"` reproduces a terminal delete, and `sync --no-purge` skips the sweep for one run.

This is what turns a sync into a backup. Make the source read-only and leave `store.purge-after` unset: a remote expunge still retires the local row, but the item and its body stay in the store, restorable, and neverest never pushes a deletion back to the server.

```toml
[accounts.backup]
imap.server = "imaps://imap.example.org:993"
imap.item.delete = false
imap.collection.delete = false
```

### Duplicated identities

A collection may hold one identity twice: two messages carrying the same `Message-ID`, two cards carrying the same `UID`. Neverest cannot tell such copies apart, so it syncs neither of them and reports the collection and every id involved, on every run until the collection holds the identity once. Which copy to keep is a decision only you can make, with your own client.

This is not an invalid mailbox, and nothing is wrong with your server. RFC 5322 binds the generator of a `Message-ID`, not what a store may hold: a copy legitimately carries the identifier of the message it copies, and a migration commonly produces such a pair. Reporting is what neverest does instead of guessing, because guessing costs mail. Propagating a delete of the copy it happened to pick removes the only copy on the other source while the first still holds the message.

### Coming from Maildir

Neverest ships no Maildir converter, and a local file store is not a sync source: the pimdir store is the local replica. Keyword storage is not standardized across Maildir consumers (info-section letters, dovecot-keywords, `X-Keywords` and `X-Label` headers), so a local migration would silently lose or mangle flags depending on which tool wrote the source tree. Initialize a fresh account and resync from the authoritative server instead: flags re-converge cleanly and the store reflects the actual server state. An existing on-disk tree is brought in through io-pimdir's conversion tooling rather than synced as a source.

## License

This project is licensed under either of:

- [MIT license](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

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
- 2026 → 2027: [NGI Zero Commons Fund](https://nlnet.nl/project/Pimalaya-pimdir/)

This program is part of Pimalaya, free software funded entirely by grants and donations. If you find it useful, consider [sponsoring](https://pimalaya.org/sponsor/) its development:

[![GitHub](https://img.shields.io/badge/-GitHub%20Sponsors-fafbfc?logo=GitHub%20Sponsors)](https://github.com/sponsors/soywod)
[![Ko-fi](https://img.shields.io/badge/-Ko--fi-ff5e5a?logo=Ko-fi&logoColor=ffffff)](https://ko-fi.com/pimalaya)
[![Buy Me a Coffee](https://img.shields.io/badge/-Buy%20Me%20a%20Coffee-ffdd00?logo=Buy%20Me%20A%20Coffee&logoColor=000000)](https://www.buymeacoffee.com/pimalaya)
[![Liberapay](https://img.shields.io/badge/-Liberapay-f6c915?logo=Liberapay&logoColor=222222)](https://liberapay.com/pimalaya)
[![thanks.dev](https://img.shields.io/badge/-thanks.dev-000000?logo=data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMjQuMDk3IiBoZWlnaHQ9IjE3LjU5NyIgY2xhc3M9InctMzYgbWwtMiBsZzpteC0wIHByaW50Om14LTAgcHJpbnQ6aW52ZXJ0IiB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciPjxwYXRoIGQ9Ik05Ljc4MyAxNy41OTdINy4zOThjLTEuMTY4IDAtMi4wOTItLjI5Ny0yLjc3My0uODktLjY4LS41OTMtMS4wMi0xLjQ2Mi0xLjAyLTIuNjA2di0xLjM0NmMwLTEuMDE4LS4yMjctMS43NS0uNjc4LTIuMTk1LS40NTItLjQ0Ni0xLjIzMi0uNjY5LTIuMzQtLjY2OUgwVjcuNzA1aC41ODdjMS4xMDggMCAxLjg4OC0uMjIyIDIuMzQtLjY2OC40NTEtLjQ0Ni42NzctMS4xNzcuNjc3LTIuMTk1VjMuNDk2YzAtMS4xNDQuMzQtMi4wMTMgMS4wMjEtMi42MDZDNS4zMDUuMjk3IDYuMjMgMCA3LjM5OCAwaDIuMzg1djEuOTg3aC0uOTg1Yy0uMzYxIDAtLjY4OC4wMjctLjk4LjA4MmExLjcxOSAxLjcxOSAwIDAgMC0uNzM2LjMwN2MtLjIwNS4xNTYtLjM1OC4zODQtLjQ2LjY4Mi0uMTAzLjI5OC0uMTU0LjY4Mi0uMTU0IDEuMTUxVjUuMjNjMCAuODY3LS4yNDkgMS41ODYtLjc0NSAyLjE1NS0uNDk3LjU2OS0xLjE1OCAxLjAwNC0xLjk4MyAxLjMwNXYuMjE3Yy44MjUuMyAxLjQ4Ni43MzYgMS45ODMgMS4zMDUuNDk2LjU3Ljc0NSAxLjI4Ny43NDUgMi4xNTR2MS4wMjFjMCAuNDcuMDUxLjg1NC4xNTMgMS4xNTIuMTAzLjI5OC4yNTYuNTI1LjQ2MS42ODIuMTkzLjE1Ny40MzcuMjYuNzMyLjMxMi4yOTUuMDUuNjIzLjA3Ni45ODQuMDc2aC45ODVabTE0LjMxNC03LjcwNmgtLjU4OGMtMS4xMDggMC0xLjg4OC4yMjMtMi4zNC42NjktLjQ1LjQ0NS0uNjc3IDEuMTc3LS42NzcgMi4xOTVWMTQuMWMwIDEuMTQ0LS4zNCAyLjAxMy0xLjAyIDIuNjA2LS42OC41OTMtMS42MDUuODktMi43NzQuODloLTIuMzg0di0xLjk4OGguOTg0Yy4zNjIgMCAuNjg4LS4wMjcuOTgtLjA4LjI5Mi0uMDU1LjUzOC0uMTU3LjczNy0uMzA4LjIwNC0uMTU3LjM1OC0uMzg0LjQ2LS42ODIuMTAzLS4yOTguMTU0LS42ODIuMTU0LTEuMTUydi0xLjAyYzAtLjg2OC4yNDgtMS41ODYuNzQ1LTIuMTU1LjQ5Ny0uNTcgMS4xNTgtMS4wMDQgMS45ODMtMS4zMDV2LS4yMTdjLS44MjUtLjMwMS0xLjQ4Ni0uNzM2LTEuOTgzLTEuMzA1LS40OTctLjU3LS43NDUtMS4yODgtLjc0NS0yLjE1NXYtMS4wMmMwLS40Ny0uMDUxLS44NTQtLjE1NC0xLjE1Mi0uMTAyLS4yOTgtLjI1Ni0uNTI2LS40Ni0uNjgyYTEuNzE5IDEuNzE5IDAgMCAwLS43MzctLjMwNyA1LjM5NSA1LjM5NSAwIDAgMC0uOTgtLjA4MmgtLjk4NFYwaDIuMzg0YzEuMTY5IDAgMi4wOTMuMjk3IDIuNzc0Ljg5LjY4LjU5MyAxLjAyIDEuNDYyIDEuMDIgMi42MDZ2MS4zNDZjMCAxLjAxOC4yMjYgMS43NS42NzggMi4xOTUuNDUxLjQ0NiAxLjIzMS42NjggMi4zNC42NjhoLjU4N3oiIGZpbGw9IiNmZmYiLz48L3N2Zz4=)](https://thanks.dev/u/gh/soywod)
[![PayPal](https://img.shields.io/badge/-PayPal-0079c1?logo=PayPal&logoColor=ffffff)](https://www.paypal.com/paypalme/soywod)
