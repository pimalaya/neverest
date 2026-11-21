---
cairn: log
change: io-family-releases
date: 2026-11-21
---

# The io-* releases of 2026-11-21

neverest moves to io-gcal 0.6.0, io-gmail 0.10, io-gpeople 0.9.0, io-http 0.9, io-msgraph 0.10.0, io-pimdir 0.19.1, io-webdav 0.13.0, pimalaya-stream 0.4.1 (io-proxy 0.3.0 underneath) and, for the live tests, io-oauth 0.8. No `[patch.crates-io]`, and no code moved.

## What landed

- Cargo.toml: the family rule cargo-006 renamed the blocking client feature `client` to `std`, `client` now gating the I/O-free coroutines. The `msgraph`, `gpeople`, `gcal` and `gmail` features name `io-<crate>/std` where they named `io-<crate>/client`. The `carddav` and `caldav` features add `io-http/client`, as src/dav/client.rs reads `io_http::rfc9112::send`, a coroutine module io-http 0.9 gates behind it. io-webdav and io-pimdir keep their features: io-webdav's TLS features already imply `std`, and io-pimdir's `client` did not move.
- Cargo.lock: the moved crates alone, no unrelated churn.
- io-pimdir 0.19.1 gives a rekeyed item in conflict, an item upgraded under a local edit and a bodiless pull the summary and sort key of the body they hold (pimdir SYNC §5, §6, §8, §9), noted in the CHANGELOG under Fixed.

## Spec

No spec moved: the summaries are io-pimdir's, and the spec names no dependency version.

## Verified

`cargo fmt --check`, `cargo build`, `cargo clippy --locked --all-targets -- -D warnings` over the CI feature sets (default, all, `rustls-ring` alone, `native-tls,imap,smtp`, and `rustls-ring` with each backend), `cargo test --all-features` (720 passed, 71 ignored: the live and the Radicale or Stalwart suites; the maddy suite tests/without_uidplus.rs ran, 2 passed) and `cargo test --no-default-features --features rustls-ring,imap,smtp` (410 passed, 50 ignored).
