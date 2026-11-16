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

- the reconcile semantics (three-way merge, checkpoints, push-outcome discipline, object dedup, the multi-source hub), the per-kind summaries and the local replica (the SQLite index, the blob store, the action queue, retention) belong in [io-pimdir](https://github.com/pimalaya/io-pimdir);
- protocol wire semantics belong in [io-imap](https://github.com/pimalaya/io-imap), [io-msgraph](https://github.com/pimalaya/io-msgraph), [io-webdav](https://github.com/pimalaya/io-webdav) and [io-smtp](https://github.com/pimalaya/io-smtp);
- configuration shape, the per-kind merge, the sync orchestration and the report live here.

The shared clap, printer and spinner primitives come from [pimalaya/cli](https://github.com/pimalaya/cli), the TOML loader and secret resolution from [pimalaya/config](https://github.com/pimalaya/config), and the TCP and TLS plumbing from [pimalaya/stream](https://github.com/pimalaya/stream).

To build against a local checkout of a Pimalaya crate, add a `<crate>.path = "../<repo>"` entry to `[patch.crates-io]`.

## Feature matrix

Every remote is a cargo feature, and every side config parses in every build: an unavailable backend fails when the sync opens that side, never at build time. Build the reduced sets when touching the gates:

```sh
cargo build --no-default-features --features rustls-ring
cargo build --no-default-features --features imap,smtp,rustls-ring
cargo build --no-default-features --features carddav,rustls-ring
cargo build --no-default-features --features caldav,rustls-ring
cargo build --all-features
```

A feature is named after the configuration table it builds (`carddav`, `caldav`, `msgraph`), never an alias of another. Code serving both DAV kinds sits under `cfg(any(feature = "carddav", feature = "caldav"))`, code for one kind under its own feature.

## Live tests

The integration tests under tests/ are ignored by default: each needs a real server, spawned by the shell script its header names.

```sh
./tests/stalwart2.sh
./tests/radicale.sh
./tests/pimdir.sh
cargo test --test submit -- --ignored
cargo test --test carddav -- --ignored --test-threads=1
```

`tests/pimdir.sh` installs the `pimdir` CLI some suites read the store with, built from the io-pimdir commit Cargo.lock locks, under target/pimdir/. Under a rootless Docker, which binds no port under 1024, start the servers with `./tests/pasta.sh servers` and run each suite as `./tests/pasta.sh cargo test …`: pasta (from passt) forwards the ports the suites name to the shifted ones.

They exist because the unit tests drive a scripted remote: the SMTP dialogue, the ETag plumbing, the conflict path and the duplicate freeze are only proven end to end here.

In CI, the offline jobs run on every push. The suites against the real Google and Microsoft accounts (tests/google.rs, tests/msgraph.rs) never run on push: they run weekly (Mondays, 05:00 UTC) and by hand with `gh workflow run tests.yml`. A release runs them by hand on the commit to tag. Each Google calendar test works in its own long-lived calendar, `pimalaya-live-neverest-<purpose>`, created when absent, emptied at the start and end of a run, and never deleted.
