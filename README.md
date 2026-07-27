<div align="center">
  <img src="./logo.svg" alt="Logo" width="128" height="128" />
  <h1>📫 Neverest</h1>
  <p>CLI to synchronize emails and contacts, written in Rust</p>
  <p>
    <a href="https://matrix.to/#/#pimalaya:matrix.org"><img alt="Matrix" src="https://img.shields.io/badge/chat-%23pimalaya-blue?style=flat&logo=matrix&logoColor=white"/></a>
    <a href="https://fosstodon.org/@pimalaya"><img alt="Mastodon" src="https://img.shields.io/badge/news-%40pimalaya-blue?style=flat&logo=mastodon&logoColor=white"/></a>
    <a href="https://pimalaya.org/sponsor/"><img alt="Sponsor" src="https://img.shields.io/badge/sponsor-pink?style=flat&logo=github-sponsors&logoColor=white"/></a>
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
  - [Contacts accounts (CardDAV)](#contacts-accounts-carddav)
  - [Retention and purging](#retention-and-purging)
- [Usage](#usage)
  - [Initializing an account](#initializing-an-account)
  - [Running a sync](#running-a-sync)
  - [Collection filters and per-side permissions](#collection-filters-and-per-side-permissions)
  - [Migrating from Maildir](#migrating-from-maildir)
  - [Checking a configuration](#checking-a-configuration)
- [License](#license)
- [AI policy](https://github.com/pimalaya/.github/blob/master/AI_POLICY.md)
- [Social](#social)
- [Contributing](./CONTRIBUTING.md)
- [Sponsoring](#sponsoring)

## Features

- Mail backend support: **IMAP**, **JMAP**, **Gmail**, **Microsoft Graph**
- Contacts backend support: **CardDAV** <sup>[rfc6352](https://www.iana.org/go/rfc6352)</sup> (requires the `carddav` feature)
- Local **pimdir store** (`pimdir.db` + a content-addressed `objects/` blob store), the single local copy an app reads
- **Simple auth** support for IMAP: anonymous, login, plain, oauthbearer, xoauth2, scram-sha-256
- **HTTP auth** support for JMAP: basic, bearer, raw header
- **OAuth 2.0 bearer token** auth for Gmail and Microsoft Graph
- **TLS** support:
  - [Rustls](https://crates.io/crates/rustls) with ring crypto
  - [Rustls](https://crates.io/crates/rustls) with aws crypto (requires `rustls-aws` feature)
  - [Native TLS](https://crates.io/crates/native-tls) (requires `native-tls` feature)
- **Discovery** support:
  - PACC <sup>[specs](https://datatracker.ietf.org/doc/html/draft-ietf-mailmaint-pacc)</sup>
  - Autoconfiguration (Thunderbird) <sup>[specs](https://wiki.mozilla.org/Thunderbird:Autoconfiguration)</sup>
  - SRV DNS lookups <sup>[rfc6186](https://datatracker.ietf.org/doc/html/rfc6186)</sup>
  - DAV service discovery <sup>[rfc6764](https://datatracker.ietf.org/doc/html/rfc6764)</sup> (with the `carddav` feature)
- **Collection filters** (include / exclude / all), applied symmetrically to both sides
- **Per-side permissions** gating `create` / `delete` on collections and items, plus `update` on flags and item content
- **Per-side connection pools** with one client per worker
- **Incremental store** at `$XDG_STATE_HOME/neverest/<account>/` (override with `store.root`)
- **Retention**: a removed item is retained, never lost, and reclaimed on a schedule (`store.purge-after`)
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

Each remote backend is a cargo feature (`imap`, `msgraph`, `carddav`), as is the
SMTP submission channel queued `submit` intents are performed through (`smtp`).
`carddav` is out of the default set until its live suite runs in CI. A side whose
backend is not compiled in reports it when the sync opens that side. With only
IMAP support:

```
cargo install --locked --git https://github.com/pimalaya/neverest.git \
  --no-default-features \
  --features imap,smtp,rustls-ring
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

Run `neverest` with no command, or run any command with no configuration file on disk, and the wizard runs. It asks for one thing, your email address, then runs provider discovery in parallel (fixed provider rules, PACC, Thunderbird Autoconfiguration, RFC 6186 SRV) and proposes every configuration it found. Pick one and it prompts the authentication mechanism the server actually advertises and its credentials, tests the connection, and offers to save the result. Declining the save (or an overwrite) prints the configuration on stdout instead, and a redirected stdout skips the prompts altogether, so `neverest > config.toml` writes the file itself. The account name is derived from your email domain; rename the TOML table if you want another.

The wizard configures a **local sync**: the discovered remote as `left`, reconciled against the local pimdir store. A remote-to-remote mirror (a second `right` side) is written by hand; see [config.sample.toml](./config.sample.toml).

Only the backends compiled into your build are proposed: IMAP (plus the SMTP send channel when discovery finds a submission server), and the Microsoft Graph API on a Microsoft account. A Gmail side takes an OAuth 2.0 bearer token rather than discoverable server settings, so it is added by hand as a `<side>.gmail.*` block.

A persistent configuration is loaded from the first valid path among:

- `$XDG_CONFIG_HOME/neverest/config.toml`
- `$HOME/.config/neverest/config.toml`
- `$HOME/.neverestrc`

See [config.sample.toml](./config.sample.toml) for a documented template covering every supported field.

Override the path with `-c <PATH>` or `NEVEREST_CONFIG=<PATH>`; multiple paths can be passed at once, separated by `:`. The first one is the base and the rest are deep-merged on top.

### Contacts accounts (CardDAV)

A CardDAV side syncs contacts rather than mail, so it pairs with another
contacts side or with the store alone: an account's two sides must agree on
their kind, and a mixed pair is refused before any connection is made. Pair
each kind with its own account; they may share a `store.root`, since a pimdir
store records each collection's kind and holds several.

```toml
[accounts.contacts]
left.carddav.server = "https://dav.example.org/"
left.carddav.auth.basic.username = "user"
left.carddav.auth.basic.password.command = ["pass", "show", "example/dav"]
```

Cards are the first **mutable** items neverest syncs: unlike a mail body, a card
is edited in place. Writes are conditional on the revision (ETag) last synced,
so a card edited on both sides is reported as a conflict and left alone rather
than overwritten, and `<side>.carddav.item.update = false` turns in-place edits
off for that side. The backend needs the `carddav` cargo feature.

### Retention and purging

The pimdir store never truly deletes an item. When its last binding vanishes (a remote expunge, propagated through the sync) the row is *retained*: hidden from the sync and from listings, but kept with its body. Reclaiming is explicit and time-based, and neverest is the sweeper: after each sync it purges every retained item older than `store.purge-after`, then reports how many items and bytes it freed.

```toml
store.purge-after = "90d"   # s, m, h, d (86400 s) or w (7 d)
```

Leaving it **unset means never purge**: retained items pile up until something reclaims them. `"0"` purges immediately, which is the old terminal-delete behaviour. There is no on/off boolean, the delay is the switch, and `neverest sync --no-purge` skips the sweep for one run.

This is what turns a sync into a **backup**. Make the remote side read-only and leave `store.purge-after` unset:

```toml
[accounts.backup]
left.imap.server = "imaps://imap.example.org:993"
left.imap.item.delete = false
left.imap.collection.delete = false
```

A remote expunge still propagates (the local row is retired), but the item and its body stay in the store, restorable, and neverest never pushes a deletion back to the server. Setting `store.purge-after` instead gives a bounded recovery window: deleted mail restorable for 90 days, then reclaimed.

An existing account can be re-configured later with `neverest configure` (or `neverest configure -a <account>` to target a non-default account): the same flow runs again, seeded with the account's current email. It replaces the `left` side and the send channel, and keeps everything else as configured, including a hand-written `right` side.

## Usage

### Initializing an account

Before the first sync each account must be initialized once:

```
neverest init [-a|--account <NAME>]
```

The account flag is optional: when omitted, the account marked `default = true` in the configuration is used.

This opens every configured side (IMAP CAPABILITY, JMAP session GET, a Graph token acquisition) so credential and network errors surface up front, then creates the empty pimdir store at `$XDG_STATE_HOME/neverest/<account>/pimdir.db` (with its `objects/` blob directory), or under `store.root` when that is set. The presence of `pimdir.db` is the single source of truth for "this account is initialized"; `sync` refuses to run when it is missing and `init` refuses to run when it is present.

### Running a sync

```
neverest sync [-a|--account <NAME>]
```

Sync walks every mailbox surviving the filter, diffs the two sides against the cached snapshot, applies the resulting hunks through per-side connection pools, then prints a report covering created / updated / deleted mailboxes, flags and messages. Pass `-d` / `--dry-run` to print the patch without applying it.

Pass `--reset` to drop the cached state before running. Without `--include-mailbox`, the entire snapshot plus every IMAP / JMAP state token is cleared; with `--include-mailbox`, only the listed mailboxes are wiped. The first post-reset sync rebuilds the snapshot via a full re-list, equivalent to first-sync semantics.

### Collection filters and per-side permissions

Collection filters declared in the configuration apply symmetrically to both sides. They can be overridden per invocation with `-m / --include-collection`, `-x / --exclude-collection`, or `-A / --all-collections` (the three flags are mutually exclusive, and the pre-v1 `--include-mailbox` spellings still work). Matching is ASCII case-insensitive: `INBOX` matches `inbox`, but non-ASCII characters (umlauts, Cyrillic, accents) must be spelled exactly as the server reports them.

Per-side permissions live under each side's backend table and gate what the sync engine is allowed to mutate on that side:

```toml
[accounts.example]
left.imap.server = "imaps://imap.example.org:993"
left.imap.collection.create = false
left.imap.collection.delete = false
left.imap.flag.update = true
left.imap.item.create = true
left.imap.item.delete = false

right.msgraph.auth.token.command = ["ortie", "-a", "msgraph", "token", "show"]
right.msgraph.item.delete = false
```

All permissions default to `true`. Setting one to `false` makes the engine treat the side as read-only for that operation: the change is kept pending rather than pushed, and the other kinds still propagate.

### Migrating from Maildir

Neverest does not ship an in-tree Maildir converter: keyword storage is not standardized across Maildir consumers (info-section letters, `dovecot-keywords`, `X-Keywords` / `X-Label` headers, …), so any local migration would silently lose or mangle flags depending on which tool wrote the source tree.

A local file store is not a sync side either: the pimdir store *is* the local replica, so the recommended path for users coming from mbsync, OfflineIMAP or a Dovecot Maildir layout is to initialize a fresh account and resync from the authoritative IMAP/JMAP server. Flags re-converge cleanly and the store reflects the actual server state.

> [!TIP]
> An existing on-disk tree is brought in through io-pimdir's conversion tooling rather than synced as a side. But since no standards exist for managing custom flags in Maildir, resyncing from IMAP/JMAP stays the recommended path.

### Checking a configuration

```
neverest check [-a|--account <NAME>]
```

Opens both sides and asks each one to list mailboxes. The operation itself is cheap; the value is in surfacing the credential, network or config errors that would otherwise only show up during a real sync.

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
