---
cairn: tasks
change: side-owned-send-channel
---

# Tasks

- [x] `config`: split `SideConfig` into a table (flattened
      `SideBackendConfig` + optional `smtp`), drop `AccountConfig.smtp`, add
      `SideConfig::new` and `sends_natively`.
- [x] `client`: open the side's backend.
- [x] `offline/driver`: resolve the channel per side, first side wins.
- [x] `wizard/discover`: return a `SideConfig` carrying the discovered SMTP
      endpoint (the `Configured` pair is gone).
- [x] `wizard/edit`: carry over the side's existing channel when a re-run
      discovers none.
- [x] `wizard/imap_smtp`, `wizard/msgraph`, `cli/check`: build sides through
      `SideConfig::new`.
- [x] Tests: the channel parses under the side, an account-level `smtp` table
      is refused, a Graph side sends natively, a mistyped backend is refused.
- [x] Docs: `config.sample.toml`, CHANGELOG, module docs (outbox, driver).
