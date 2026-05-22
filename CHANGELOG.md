# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **BREAKING**: rewrote neverest on top of the new `io-*` ecosystem (`io-email`, `io-imap`, `io-jmap`, `io-maildir`) and `pimalaya-cli` / `pimalaya-config` / `pimalaya-stream`. The old `email-lib` (pimalaya/core) dependency is gone; the sync engine now lives inside `neverest::sync` and drives both sides through `io_email::client::EmailClientStd`.
- **BREAKING**: renamed `folder` to `mailbox` everywhere (config keys, CLI flags). `--include-folder` becomes `--include-mailbox` / `-m`; `mailbox.filters` replaces `folder.filters`; the per-side `mailbox`/`flag`/`message` permission tables now live directly under each side instead of under `left.folder.permissions`.
- **BREAKING**: side configuration moved from `left.backend.type = "imap"` to `left.imap.server = "..."` (likewise for `jmap`, `maildir`). Exactly one of the three sub-tables must be set per side.
- Tokio runtime removed — neverest is now synchronous (io-* clients use `std::net`).
- Sync cache moved to `$XDG_CACHE_HOME/neverest/<account>/state.json` (JSON instead of the old binary format).
- Error reporting now uses `anyhow` + `pimalaya_cli::error::ErrorReport`; `color-eyre`'s `--debug` / `--trace` flags are replaced by `--log-level`.

### Added

- **JMAP** backend support via `io-jmap`.

### Removed

- **BREAKING**: dropped the **Notmuch** backend. No replacement exists in the new `io-*` ecosystem yet; track upstream for a future `io-notmuch`.
- Removed `email-lib`, `pimalaya-tui`, `oauth-lib`, `secret-lib`, `console`, `color-eyre`, `async-trait`, `tokio`, `once_cell` dependencies.

### Refactored IMAP auth config API

  The IMAP auth config option is now explicit, in order to improve error messages:

  ```toml
  # before
  right.backend.password.cmd = "pass show example"

  # after
  right.backend.auth.type = "password"
  right.backend.auth.cmd = "pass show example"
  ```

## [1.0.0-beta] - 2024-04-15

### Added

- Added `--debug` as an alias for `RUST_LOG=debug`.
- Added `--trace` as an alias for `RUST_LOG=trace`.
- Added notes about `--debug` and `--trace` when error occurs.
- Added `left|right.folder.aliases` to define custom folder aliases.

### Changed

- Replaced `anyhow` by [`color-eyre`](https://crates.io/crates/color-eyre) for better error management.
- Replaced `log` by [`tracing`](https://crates.io/crates/tracing) for better log management.
- Renamed `folder.filter` to `folder.filters` in order to match lib types.
- Renamed `envelope.filter` to `envelope.filters` in order to match lib types.
- Renamed `check` command to `doctor`.

## [0.1.0] - 2024-04-10

### Added

- Initiated the project from [Himalaya CLI](https://github.com/pimalaya/himalaya).

[Unreleased]: https://github.com/pimalaya/neverest/compare/v1.0.0-beta...HEAD
[1.0.0-beta]: https://github.com/pimalaya/neverest/compare/v0.1.0...v1.0.0-beta
[0.1.0]: https://github.com/pimalaya/neverest/compare/root...v0.1.0
