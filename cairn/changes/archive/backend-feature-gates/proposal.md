---
cairn: change
id: backend-feature-gates
status: landed
created: 2026-08-07
---

# Every remote is a cargo feature

## Why

IMAP was gated behind an `imap` cargo feature; the Microsoft Graph backend and
the SMTP submission channel that arrived later were not. Both are remotes (one
enumerates and fetches, the other submits), so both linked unconditionally: a
build wanting IMAP only still pulled io-msgraph, io-smtp and their TLS stacks.

The `imap` gate was also broken in practice. Two ungated call sites reached into
the optional crate (`io_imap::client::default_alpn` as a serde default in
`config.rs`, the same call in the wizard's `imap_to_config`), so
`--no-default-features` failed to compile and nobody noticed.

Both optional-in-spirit crates also hardcoded `features = ["rustls-ring"]`, so
`--features native-tls` silently left Graph and SMTP on rustls.

## What

- `msgraph` and `smtp` join `imap` as cargo features; all three are in the
  default set, so a default build is byte-for-byte what it was. io-msgraph and
  io-smtp become `optional = true` and take their TLS provider from neverest's
  own `native-tls` / `rustls-aws` / `rustls-ring` features (and `vendored`)
  rather than pinning rustls-ring.
- Every feature combination compiles, including one with no backend at all: a
  missing backend is a runtime error, never a build one. The config surface
  stays whole in every build too. Every `SideConfig` variant still parses; an
  unavailable backend fails when the side is *opened*, exactly as the JMAP and
  Gmail variants already did. This keeps one config schema and one failure mode
  across builds instead of one per feature combination.
- The two ungated `io_imap` reaches are removed by making `alpn` optional on
  the IMAP and SMTP configs. Unset means "the backend's own default", resolved
  at the (gated) connect site from `io_imap::client::default_alpn` and
  `SmtpClientStd::default_alpn`, so the libraries keep owning their defaults and
  no value is frozen into the config the wizard writes.
- The Outbox splits along the same line: the queue (`OUTBOX`, `is_outbox`,
  `OutboxMeta`, `queued_sends`) is always compiled in, only the channels
  draining it are gated. A build with neither channel accumulates queued sends
  and warns instead of flushing.

## Scope / non-goals

- The wizard stays ungated. It runs on pimalaya-cli's IMAP/JMAP prompts, not on
  io-imap, so it compiles in every build; a Graph-only build simply gets a
  wizard that produces a side it cannot open, which is the pre-existing JMAP and
  Gmail behaviour.
- JMAP and Gmail get no feature, having no backend to gate yet.
- The `imap` combinations could not be verified against the published io-imap
  0.3.1: `imap/backend.rs` calls `fetch_bodies_stream`, which exists only on
  io-imap master. Verified against a local path patch, reverted afterwards.
