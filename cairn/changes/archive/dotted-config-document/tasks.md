---
cairn: tasks
change: dotted-config-document
---

# Tasks

- [x] `config`: render through `config_toml::to_string` in `Config::write`.
- [x] `wizard/discover`: render the stdout document the same way, so a saved
      file and a printed one are byte-for-byte identical.
- [x] `config`: add the `is_default` / `is_default_http_alpn` predicates and
      skip the defaulted fields (account `default`, side permissions,
      `pool-size`, `mailbox.filter`, `mailbox.alias`, HTTP `alpn`, `starttls`).
- [x] `config`: derive `PartialEq` where the predicate needs it (the three
      permission structs, `MailboxFilter`).
- [x] Test: a wizard-shaped Graph account renders as one header plus dotted
      keys, with no defaulted value.
- [x] Docs: CHANGELOG.
