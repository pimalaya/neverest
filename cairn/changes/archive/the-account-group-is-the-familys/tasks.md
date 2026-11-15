---
cairn: tasks
change: the-account-group-is-the-familys
---

# Tasks

- [x] pimalaya-cli 0.5.1 then 0.5.2, the lock moving that crate alone
- [x] src/cli/account.rs: `AccountCommand` (`list`, `check`), `AccountListCommand`; src/cli/main.rs: the group, `check` hidden; the welcome names `account check`
- [x] src/config.rs: `BACKENDS`, `is_built`, `AccountConfig::backends`
- [x] src/cli/check.rs: `BackendCheck` for the send channel and the row's keys, coded through `error::coded`
- [x] src/json_schema.rs: `neverest-account-list`, `neverest-account-check`, `neverest-check`
- [x] Tests: tests/account.rs (`account list` rows and text, `account check` and `check` alike, `check` hidden), `an_account_lists_the_backends_its_endpoints_write`
- [x] README, MIGRATION, CHANGELOG; fold into cairn/spec/sync.md, log
