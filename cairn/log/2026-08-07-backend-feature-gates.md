---
cairn: log
change: backend-feature-gates
landed: 2026-08-07
---

# Every remote is a cargo feature

Microsoft Graph and SMTP submission joined IMAP behind cargo features. All three
(`imap`, `msgraph`, `smtp`) are in the default set, so a default build is what it
was; what changes is that a slimmer one is now possible and correct.

**The `imap` gate was broken before this change.** Two ungated call sites reached
into the optional crate — `io_imap::client::default_alpn` as a serde default in
`config.rs`, the same call in the wizard's `imap_to_config` — so
`--no-default-features` had never compiled. Fixed by making `alpn` an `Option` on
both the IMAP and the SMTP config: unset means the backend crate's own default,
resolved at the connect site (`io_imap::client::default_alpn`,
`SmtpClientStd::default_alpn`), `[]` still skips ALPN. The wizard now writes no
`alpn` at all rather than freezing today's value into the user's config. This is
a behaviour change for SMTP, which previously offered no ALPN and now offers the
`smtp` token RFC 7595 registers; `smtp.alpn = []` restores the old behaviour, and
the CHANGELOG says so.

**Deps** (`Cargo.toml`): io-msgraph and io-smtp became `optional = true` and lost
their hardcoded `features = ["rustls-ring"]`, which had made
`--features native-tls` silently leave Graph and SMTP on rustls. All three
optional crates now take the TLS provider (and `vendored`) from neverest's own
features through the `?/` syntax. The redundant `pimalaya-cli/imap` entry left the
`imap` feature: the base dependency already carries it, since the wizard runs on
pimalaya-cli's prompts and stays ungated.

**Config stays whole in every build.** Only the client and channel code is gated,
never the config schema: every `SideConfig` variant still parses and an
unavailable backend fails when the side is *opened*, exactly as the JMAP and
Gmail variants already did. One schema across builds beats one per feature
combination.

**The Outbox split along the queue/channel line** (`offline/outbox.rs`): the queue
itself (`OUTBOX`, `is_outbox`, `OutboxMeta`, `queued_sends`) is unconditional, and
only `SendChannel`, `connect_smtp`, `flush`, `send_one` and the SMTP path helpers
are gated. `driver::flush_outbox` grew an `open_send_channel` that resolves
whichever channel is compiled in; a build with neither accumulates queued sends
and warns.

**Every combination compiles, including a build with no backend at all**: a
missing backend is a runtime error, never a build one, matching how an
unavailable JMAP or Gmail side already behaved. `open` refuses such a side
first, so `Client` only needs to stay inhabited: it carries a never-constructed
`Unavailable` variant whose arms report `NO_BACKEND`.

Gating exposed seven spots that are genuinely dead in some combinations, each
marked `cfg_attr(..., allow(dead_code))` on its narrowest condition rather than
deleted or unconditionally allowed: `Client`'s parameter union,
`SaslConfig::try_into_sasl`, `remote::encode_checkpoint`,
`envelope::normalize_message_id` and `Flag::iana` (IMAP/Graph-only),
`OutboxMeta`'s envelope fields (SMTP-only) and `TlsConfig::into_tls` (needs any
connecting backend).

Verified: `rustls-ring` alone, `msgraph`, `smtp`, `msgraph,smtp`, `imap`,
`imap,smtp`, `imap,msgraph` and `imap,msgraph,smtp` all check clean with no
warnings; 31 tests pass; fmt clean.

**Known blocker, unrelated to this change:** the published io-imap 0.3.1 has no
`fetch_bodies_stream`, which `imap/backend.rs` calls, so every `imap` combination
fails against crates.io. It exists on io-imap master (`a75b3c6`, unreleased); the
path patches that used to hide this were removed from `Cargo.toml`. The `imap`
combinations above were verified against a temporary local path patch, reverted
afterwards. Neverest needs an io-imap release before it builds.

Spec updated: `sync` (ADDED: "Every remote backend is a cargo feature", "A
backend owns its ALPN default"; MODIFIED: "The Outbox is local-only and flushes
through the send channel" now states that only the channels are gated).
