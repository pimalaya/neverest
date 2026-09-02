# Contributing guide

Thank you for investing your time in contributing to Neverest.

Whether you are a human or an AI agent, read these in order before touching the code:

1. the [Pimalaya README](https://github.com/pimalaya) for what the project is and how its repositories stack;
2. the [Pimalaya CONTRIBUTING](https://github.com/pimalaya/.github/blob/master/CONTRIBUTING.md) guide (Nix environment, build and check commands, dependency overrides, commit style), which chains to the shared architecture and guidelines;
3. the inline header documentation in src/main.rs: it is the architecture document of this crate, covering the topology mismatch the engine resolves, the kind seam and the module layout;
4. the cairn/ folder for the development history and living plans (the Cairn convention: spec/, changes/, log/).

Everything below documents only what differs from the Pimalaya standards.

## Where changes belong

Neverest is an application: it writes no protocol and no storage logic of its own, so most fixes land upstream rather than here. Triage before patching:

- reconcile semantics (three-way merge, checkpoints, push-outcome discipline, object dedup, the multi-source hub) belong in [io-replica](https://github.com/pimalaya/io-replica);
- the local replica (the SQLite index, the blob store, the action queue, retention) belongs in [io-pimdir](https://github.com/pimalaya/io-pimdir);
- protocol wire semantics belong in [io-imap](https://github.com/pimalaya/io-imap), [io-msgraph](https://github.com/pimalaya/io-msgraph), [io-webdav](https://github.com/pimalaya/io-webdav) and [io-smtp](https://github.com/pimalaya/io-smtp);
- service discovery consumed by the wizard belongs in [io-pim-discovery](https://github.com/pimalaya/io-pim-discovery);
- configuration shape, the per-kind derivations, the sync orchestration and the report live here.

The shared clap, printer, prompt and spinner primitives come from [pimalaya/cli](https://github.com/pimalaya/cli), the TOML loader and secret resolution from [pimalaya/config](https://github.com/pimalaya/config), and the TCP and TLS plumbing from [pimalaya/stream](https://github.com/pimalaya/stream).

To build against a local checkout of a Pimalaya crate, add a `<crate>.path = "../<repo>"` entry to `[patch.crates-io]`.

## Feature matrix

Every remote is a cargo feature, and every side config parses in every build: an unavailable backend fails when the sync opens that side, never at build time. Build the reduced sets when touching the gates:

```sh
cargo build --no-default-features --features rustls-ring
cargo build --no-default-features --features imap,smtp,rustls-ring
cargo build --all-features
```

`carddav` is out of the default set until its live suite runs in CI.

## Live tests

The integration tests under tests/ are ignored by default: each needs a real server, spawned by the shell script its header names.

```sh
./tests/stalwart2.sh
cargo test --test submit -- --ignored
cargo test --test relay -- --ignored
cargo test --test duplicates -- --ignored

./tests/radicale.sh
cargo test --all-features --test carddav -- --ignored
```

They exist because the unit tests drive a scripted remote: the SMTP dialogue, the ETag plumbing, the conflict path and the duplicate freeze are only proven end to end here.
