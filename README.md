<div align="center">
  <img src="./logo.svg" alt="Logo" width="128" height="128" />
  <h1>📫 Neverest</h1>
  <p>CLI to synchronize mails, contacts and calendars</p>
  <p>
    <a href="https://matrix.to/#/#pimalaya:matrix.org"><img alt="Matrix" src="https://img.shields.io/badge/chat-%23pimalaya-blue?style=flat&logo=matrix&logoColor=white"/></a>
    <a href="https://fosstodon.org/@pimalaya"><img alt="Mastodon" src="https://img.shields.io/badge/news-%40pimalaya-blue?style=flat&logo=mastodon&logoColor=white"/></a>
    <a href="https://pimalaya.org/sponsor/"><img alt="Sponsor" src="https://img.shields.io/badge/sponsor-pink?style=flat&logo=github-sponsors&logoColor=white"/></a>
  </p>
</div>

```sh
$ neverest sync
```

```text
[1/1] Syncing account personal
Resolved credentials
Opened carddav
Listed 1 collection(s) on carddav
Scanning carddav (0/1)
Scanning carddav (1/1)

Collection patches (1):
 - fetch collection book from carddav

Item patches (2):
 - fetch item ada in book from carddav
 - fetch item alan in book from carddav

Search index:
 - folded 2 change(s), 2 new body(ies), current

Account personal synchronized: 3 hunks
```

## Table of contents

- [Features](#features)
- [Installation](#installation)
- [Configuration](#configuration)
- [Usage](#usage)
- [Alternatives](#alternatives)
- [AI policy](https://github.com/pimalaya/.github/blob/master/AI_POLICY.md)
- [License](#license)
- [Social](#social)
- [Contributing](./CONTRIBUTING.md)
- [Sponsoring](#sponsoring)

## Features

- **Mail, contacts and calendars** over IMAP, Gmail, Microsoft Graph, CardDAV, CalDAV and the Google APIs
- **One local pimdir store** the offline frontends read, every account and kind in it
- **Two-way, one-way or migration** syncs, picked by an account's sources and targets
- **Three-way merge** of cards and events, a real conflict parked for a decision, never lost
- **Retention**: a removed item keeps its body in the store until a purge you schedule
- **Queued writes** of himalaya, cardamum and calendula applied offline, sends included
- **Search index** of the store kept current for the frontends to query
- **JSON output** of every result and of a sync's progress, described by `neverest json-schema`

## Installation

<details>
  <summary>Debian, Ubuntu</summary>

  Neverest is not in the official repositories yet, but the signed APT repository of [pkg.pimalaya.org](https://pkg.pimalaya.org) ships the latest release with its man pages and shell completions. Add the repository once:

  ```sh
  sudo install -d -m 0755 /etc/apt/keyrings
  curl -fsSL https://pkg.pimalaya.org/pimalaya.asc | sudo tee /etc/apt/keyrings/pimalaya.asc >/dev/null
  sudo tee /etc/apt/sources.list.d/pimalaya.sources >/dev/null <<EOF
  Types: deb
  URIs: https://pkg.pimalaya.org/apt
  Suites: stable
  Components: main
  Signed-By: /etc/apt/keyrings/pimalaya.asc
  EOF
  sudo apt update
  ```

  Then install Neverest:

  ```sh
  sudo apt install neverest
  ```
</details>

<details>
  <summary>Fedora, RHEL, Rocky Linux, AlmaLinux</summary>

  Neverest is not in the official repositories yet, but the signed DNF repository of [pkg.pimalaya.org](https://pkg.pimalaya.org) ships the latest release with its man pages and shell completions:

  ```sh
  sudo curl -fsSL -o /etc/yum.repos.d/pimalaya.repo https://pkg.pimalaya.org/pimalaya.repo
  sudo dnf install neverest
  ```
</details>

<details>
  <summary>Homebrew (macOS, Linux)</summary>

  Neverest is not in homebrew-core yet, but the [pimalaya/tap](https://github.com/pimalaya/homebrew-tap) formula installs the latest release with its man pages and shell completions:

  ```sh
  brew install pimalaya/tap/neverest
  ```
</details>

<details>
  <summary>Pre-built binary</summary>

  For other systems, Neverest can be installed with the installer:

  *As root:*

  ```sh
  curl -sSL https://raw.githubusercontent.com/pimalaya/neverest/master/install.sh | sudo sh
  ```

  *As a regular user:*

  ```sh
  curl -sSL https://raw.githubusercontent.com/pimalaya/neverest/master/install.sh | PREFIX=~/.local sh
  ```

  These commands install the latest [release](https://github.com/pimalaya/neverest/releases), built with the default cargo features. For a build of `master`, check out the [releases](https://github.com/pimalaya/neverest/actions/workflows/releases.yml) GitHub workflow and look for the *Artifacts* section.
</details>

<details>
  <summary>Cargo</summary>

  ```sh
  cargo install --locked neverest
  ```

  For a build of `master`:

  ```sh
  cargo install --locked --git https://github.com/pimalaya/neverest.git
  ```
</details>

<details>
  <summary>Nix</summary>

  ```sh
  nix profile install github:pimalaya/neverest
  ```
</details>

<details>
  <summary>Sources</summary>

  Requires [Rust](https://www.rust-lang.org/tools/install) 1.89 or later:

  ```sh
  git clone https://github.com/pimalaya/neverest
  cd neverest
  cargo build --release --locked
  ```

  The binary lands at target/release/neverest.
</details>

## Configuration

A TOML configuration is loaded from every existing path among the following, each one deep-merged on top of the ones before:

- `$XDG_CONFIG_HOME/neverest/config.toml`
- `$HOME/.config/neverest/config.toml`
- `$HOME/.neverestrc`

Override the path with `-c <PATH>`, repeated to merge several files.

> [!TIP]
> See [config.sample.toml](./config.sample.toml) for a complete configuration of one example account with commented fields; rename the account and uncomment what you need.

## Usage

**Step 1**: once your first account is configured, check that every endpoint answers:

```sh
neverest account check
```

**Step 2**: initialize the account in the store:

```sh
neverest init
```

**Step 3**: preview the first sync, which writes nothing:

```sh
neverest sync --dry-run
```

**Step 4**: if the preview looks right, run it:

```sh
neverest sync
```

Every command covers every account unless `-a` names some, and its `--help` details its flags, exit codes and JSON output. More use cases:

```sh
# Sync the inbox of the work account only
neverest sync -a work -m inbox

# Apply what himalaya, cardamum or calendula queued, offline
neverest sync --queue-only

# Build the store's search index whole, for a large first sync
neverest sync --index-only

# Settle what both sides changed differently, through the configured merger
neverest conflict list
neverest conflict resolve <id> -a work --interactive
```

## Alternatives

- [mbsync](https://isync.sourceforge.io/): syncs IMAP mailboxes with a Maildir, mail only
- [OfflineIMAP](https://github.com/OfflineIMAP/offlineimap3): syncs IMAP mailboxes with a Maildir, in Python
- [vdirsyncer](https://github.com/pimutils/vdirsyncer): syncs CardDAV and CalDAV collections with a vdir, contacts and calendars only
- [pimsync](https://pimsync.whynothugo.nl/): the Rust successor of vdirsyncer, contacts and calendars only

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
