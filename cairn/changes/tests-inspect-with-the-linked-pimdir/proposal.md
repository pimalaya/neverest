---
cairn: change
id: tests-inspect-with-the-linked-pimdir
status: active
created: 2026-10-18
---

# Tests inspect the store with the pimdir neverest links

## Why

The local Radicale suites read the store through the `pimdir` operator CLI taken from PATH: `Command::new("pimdir")` in tests/caldav.rs:255, tests/carddav.rs:615, tests/calendars.rs:520 and tests/submit.rs:422, each failing with `spawn pimdir (cargo install --path ../io-pimdir --features cli)` when it is absent. In the A2 audit run (2026-10-11) all seven such tests first failed on `NotFound`, then passed with a local io-pimdir/target/debug prepended to PATH, a tree newer than the io-pimdir neverest pinned. The CLI that inspects the store is then not the engine that wrote it (neverest links io-pimdir through `[patch.crates-io]`, Cargo.toml:89, rev 3b4bed8): a schema or rendering change between them passes or fails a test for the wrong reason.

## What

The tests read the store through the io-pimdir neverest links, never a binary found on PATH:

- either a test helper over the io-pimdir library (the store opened read-only, items listed and rendered as the assertions need), shared by the four suites;
- or, if the CLI's rendering is what the tests mean to check, the binary named by an environment variable and its version checked against the linked io-pimdir's before use, a mismatch failing with both versions named.

The first is preferred: it needs no install step and cannot skew.

## Not in scope, described

- Running the Radicale suites in CI: they need a Docker Radicale and are local today.
