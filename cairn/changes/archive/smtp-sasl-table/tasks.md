---
cairn: tasks
change: smtp-sasl-table
---

# Tasks

- [x] Replace `SmtpConfig::{login,password}` with `sasl`, ordered as `ImapConfig`.
- [x] Widen the `SaslConfig::try_into_sasl` gate to `any(imap, smtp)`.
- [x] Resolve a bare `smtp.server` authority as `smtps://` in `connect_smtp`,
      and build the SASL from the table.
- [x] Add `io-smtp/scram` to the `smtp` feature.
- [x] Wizard: reuse the IMAP table whatever it names, prompt a mechanism per
      service, keep the unauthenticated relay reachable.
- [x] Cover the new shape and the refusal of the flat one.
- [x] config.sample.toml, CHANGELOG.md, MIGRATION.md.
- [x] Fold the delta into cairn/spec/sync.md and log it.
