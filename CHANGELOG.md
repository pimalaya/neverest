# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Refactored IMAP auth config API

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
