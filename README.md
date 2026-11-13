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
> Neverest is `v0.x`: expect breaking changes between releases until it stabilises.

## Table of contents

- [Features](#features)
- [Installation](#installation)
- [Configuration](#configuration)
- [Usage](#usage)
- [Migration](./MIGRATION.md)
- [AI policy](https://github.com/pimalaya/.github/blob/master/AI_POLICY.md)
- [License](#license)
- [Social](#social)
- [Contributing](./CONTRIBUTING.md)
- [Sponsoring](#sponsoring)

## Features

- **PIM domain** support: **mail** via IMAP and Microsoft Graph, **contacts** via CardDAV <sup>[rfc6352](https://www.iana.org/go/rfc6352)</sup>, Microsoft Graph and Google People, **calendar** via CalDAV <sup>[rfc4791](https://www.iana.org/go/rfc4791)</sup>, Microsoft Graph and Google Calendar, one account syncing several at once
- **Local pimdir store** <sup>[specs](https://github.com/pimalaya/pimdir)</sup>: the single local copy an app reads, holding every domain an account syncs
- **Retention**: a removed item is kept, never lost, and reclaimed on a schedule
- **Relay** mode: a body crossing two IMAP servers is streamed server-to-server, never stored
- **Queued submission**: a message a frontend enqueued leaves through its source's send channel
- **Search index**: the store's full-text index <sup>[specs](https://github.com/pimalaya/pimdir)</sup>, kept current at every sync for the frontends to query
- **Auth** support: anonymous, login, plain, oauthbearer, xoauth2, scram-sha-256 for IMAP; basic and bearer for CardDAV and CalDAV; bearer for Microsoft Graph, Google People and Google Calendar
- **TLS** support: [Rustls](https://crates.io/crates/rustls) with ring or aws crypto (`rustls-aws` feature), [Native TLS](https://crates.io/crates/native-tls) (`native-tls` feature)
- **TOML configuration** with multi-account support, and **JSON** output via `--json`, described by `neverest json-schema`

> [!TIP]
> Neverest is written in [Rust](https://www.rust-lang.org/) and uses [cargo features](https://doc.rust-lang.org/cargo/reference/features.html) to gate backend support: CardDAV and CalDAV need `dav`, Microsoft Graph mail, contacts and calendars need `msgraph`, Google contacts `gpeople`, Google calendars `gcal` and Gmail `gmail`. The default feature set is declared in [Cargo.toml](./Cargo.toml). JMAP sources configure but have no backend yet.
>
> The default `vendored` feature builds SQLite from source; leave it out (`--no-default-features --features …`) to link the system one.
>
> The default `search` feature builds the store's search index, which frontends such as himalaya search through and never build themselves; leave it out only where no frontend searches the store.

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

Neverest writes no configuration for you: copy [config.sample.toml](./config.sample.toml) to one of the paths above and uncomment what you need. A command finding no configuration file fails naming the path it looked for, and a bare `neverest` prints the help.

Every account shares **one pimdir store**, at `neverest/` under your state directory (`` on Linux, `~/Library/Application Support` on macOS, `%LOCALAPPDATA%` on Windows) unless it names its own `store.root`. Each account is fed by one or more named **sources**, each a remote, and it may hold several kinds at once; its collections are grouped under its name, so a frontend reads one account of the store through its own account key (`pimdir.account` in himalaya, calendula and cardamum). What an account does is its arity plus two flags: with no `targets` every source syncs into the local store, the offline replica; with them the source is copied to each target.

`one-way` makes the sources authoritative, so the other side's changes are overwritten rather than merged, and `retain` says whether the store keeps bodies or is only the ledger it has to be anyway. Sources never meet, so several cache side by side for a frontend to union at display time.

A collection one endpoint no longer lists is deleted on the other only where that endpoint sets `collection.delete = true`. It is off by default: a missing collection is the only sign IMAP and DAV give of a deletion, and a listing can leave out one nobody deleted. The same setting lets a frontend ask neverest to delete a collection on that server (pimdir's `collection-delete` intent, `himalaya pimdir mailbox delete`); renaming one (`collection-rename`) needs no setting.

## Usage

Every command carries its own `--help`, the source of truth for its flags and syntax.

```sh
neverest init
neverest sync --dry-run
neverest sync -a <account> --include-collection INBOX
neverest check
neverest drain
neverest index
neverest conflict list
neverest conflict resolve <id> -a <account> --interactive
```

Every command runs over every account, one after the other, unless `-a` names some (it is repeatable). Each account's outcome is its own: one that fails is reported, the next one still runs, and the command exits 4. The store is locked for the whole run, so two accounts of one store never sync at once; give an account that needs its own schedule its own `store.root`. `conflict show`, `conflict resolve` and `sync --reset` act on one account, which `-a` names when there are several. `index` builds the store of the accounts it runs over, each store once. `default = true` on an account is ignored, with a warning.

An account is initialized once, which opens every source so credential and network errors surface up front, then adds the account to the store, creating the store when there is none. `sync` refuses to run an account without it, and `init` leaves an initialized account as it is. `sync --reset -a <account>` drops that account's collections before a run, rebuilding them as a first sync would, every other account of the store left as it was. It refuses while they hold a queued or parked action, which `drain` applies or its frontend cancels first, and while they hold other work no server has (conflicts, unpushed edits, mail a replica keeps after the server expunged it), listing that work; `--reset --force` drops that other work with the collections. A reset is the whole account's, so it is refused beside `--dry-run`, `-m`, `-x` and `-s`.

What himalaya, calendula and cardamum write is queued in the store, and only neverest applies it. A sync does so first, before reaching any server; `drain` does it alone, offline, so a frontend reads a created item back at once, the next sync pushing it.

neverest also keeps the store's search index (index.db), which the frontends query and only the store's owner builds. A sync refreshes it last, for at most `store.index-timeout` (60 seconds by default, `"0"` turning it off), and a drain for 2 seconds; a large first build spreads over several runs, each going on where the last stopped, or `index` builds it whole at once. The report says how far it got under `index`; an index failure is a warning and never changes the exit code. After a reset, the next refresh drops what the account's collections held. The index reads floating times and all-day dates in your time zone, `TZ` else the system's, UTC when it has no IANA name; changing it re-expands every calendar item at the next refresh.

### Conflicts

A card or an event edited on both sides is merged against the base the last sync agreed on, so two people touching different fields cost nobody a decision. What no merge settles is both sides setting one field two ways: the item parks, everything else keeps syncing, and the run exits 3. A run that could not reach a source, or apply part of its work, exits 4, and the next run picks it up. Exit 1 is a failure, which `--json` prints as a failure object, and exit 2 a command line neverest refused, which `--json` prints as a failure object coded `usage-invalid`.

Neverest raises no desktop notification of its own. Every run warns in the log, and `--json` carries, in each account's report, the two numbers a notifier needs: `conflicts` is what this run marked, `outstandingConflicts` what the store holds waiting for that account. Testing the first notifies on entry, once, with no state of its own to keep:

```sh
neverest sync --json | jq -e 'select(has("event") | not) | [.accounts[].report.conflicts // [] | length] | add > 0' >/dev/null \
  && notify-send "neverest" "an item needs a decision"
```

A wrapper reading that report can name the item, its collection and its side, which a fixed summary and body could not.

Before its result, `sync --json` prints its progress, one line per event, each carrying `event` (`neverest json-schema neverest-sync.events` describes them), `account-started` announcing each account, so a frontend can show how far a long run got; the result is the last line, listing each account's report under `accounts`.

Deciding is a command and never a run. `conflict list` names what is waiting, `conflict show <id>` prints the three bodies, and `conflict resolve <id>` settles one, either by taking a side with `--prefer-local` or `--prefer-remote` or by handing the bodies to the merger `conflict.merger` names.

A settled body has to be a body of that item, so one no parser reads and one stating another `UID` are both refused. A decision is refused when the remote moved while it was being made, and nothing here holds the store, so a sync stays free to run while a merger is open.

Exit 3 is wider than conflicts: the run reconciled its collections but left something waiting for a person, be it a parked conflict, a duplicate `UID` the other side refuses, a write it would not take, a send the server refused for good, a queued action that could not be applied, a listing that left out most of a collection, or a change withheld over a body the store cannot serve. The report names which. A write that did not land is never counted, so `already in sync` means the run wrote nothing.

A listing that leaves out more than half of at least 20 items a collection held drops nothing: it reads as a server answering short, and the report names it under `absences`. Once the server is known to hold no more, `sync --accept-absences` drops them, for that run only.

A server that lists at most a number of the messages a folder holds (Gmail over IMAP with "Limit IMAP folders to contain no more than this many messages" set) leaves the same messages out of every listing, often fewer than half. Set `collection.capped = true` on that source: what its listings leave out is then kept and counted in the report under `unlisted`, and only a removal the server states itself deletes. Gmail's IMAP states none, so a message deleted there stays in the store while the key is set; the `gmail` backend, which lists every message, has no such limit.

### Retention and backup

The store never truly deletes an item: when its last binding vanishes the row is retained, hidden from the sync and from listings but kept with its body. After each sync neverest purges every retained item older than `store.purge-after`. Unset means never purge, `"0"` reproduces a terminal delete, and `sync --no-purge` skips one run.

This is what turns a sync into a backup. Make the source read-only and leave `store.purge-after` unset: a remote expunge still retires the local row, but the item and its body stay in the store, restorable, and neverest never pushes a deletion back to the server.

```toml
[accounts.backup]
imap.server = "imaps://imap.example.org:993"
imap.item.delete = false
imap.collection.delete = false
```

### Duplicated identities

A collection may hold one identity twice: two messages carrying the same `Message-ID`, two cards the same `UID`. Neverest cannot tell such copies apart, so it syncs neither and reports every id involved, on every run until the collection holds the identity once. Which copy to keep is yours to decide.

Nothing is wrong with your server: RFC 5322 binds the generator of a `Message-ID`, not what a store may hold, and a migration commonly produces such a pair. Guessing costs mail, since propagating a delete of the copy it happened to pick removes the only copy on the other source.

### Coming from Maildir

Neverest ships no Maildir converter, and a local file store is not a sync source: the pimdir store is the local replica. Keyword storage is not standardized across Maildir consumers, so a local migration would silently lose or mangle flags depending on which tool wrote the source tree.

Initialize a fresh account and resync from the authoritative server instead: flags re-converge cleanly and the store reflects the actual server state. An existing on-disk tree is brought in through io-pimdir's conversion tooling rather than synced as a source.

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
