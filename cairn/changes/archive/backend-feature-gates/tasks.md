---
cairn: tasks
change: backend-feature-gates
---

# Tasks

- [x] `Cargo.toml`: `msgraph` / `smtp` features, io-msgraph and io-smtp
      `optional = true`, TLS + `vendored` propagation to all three optional
      crates, drop the redundant `pimalaya-cli/imap` from the `imap` feature.
- [x] `main.rs`: gate `mod msgraph`.
- [x] `client.rs`: gate the `Client::Msgraph` variant, every dispatch arm and
      the `open` arm; resolve the IMAP ALPN default from io-imap; keep the enum
      inhabited with a never-constructed `Unavailable` so a backend-less build
      compiles and fails when it opens a side.
- [x] `config.rs`: `ImapConfig.alpn` / `SmtpConfig.alpn` become `Option`, so no
      ungated call reaches into io-imap; `SaslConfig` is IMAP-only.
- [x] `wizard/account.rs`: write no `alpn`, leaving the default to io-imap.
- [x] `offline/outbox.rs`: keep the queue unconditional, gate `SendChannel`,
      `connect_smtp`, `flush`, `send_one` and the SMTP helpers; resolve the SMTP
      ALPN default from io-smtp.
- [x] `offline/driver.rs`: `open_send_channel` per compiled channel; the
      no-channel build warns and leaves the queue put.
- [x] Feature matrix green: `msgraph`, `msgraph,smtp`, `imap`, `imap,smtp`,
      `imap,msgraph`, `imap,msgraph,smtp` (the `imap` ones against a temporary
      local io-imap path patch, reverted); no-backend build correctly refused;
      31 tests pass; fmt clean.
- [x] Docs: CHANGELOG (gates + the SMTP ALPN behaviour change), README install
      example, `config.sample.toml` `smtp.alpn`.
- [x] Fold `delta.md` into `cairn/spec/sync.md`; add `cairn/log`; land.
