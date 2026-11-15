---
cairn: log
change: the-account-group-is-the-familys
date: 2026-11-15
---

# The account group is the family's

Line N4 of the CLI alignment round of 2026-11-15 (audit finding F1, decision D1), and the move to pimalaya-cli 0.5.1 then 0.5.2.

## What landed

- Cargo.lock: pimalaya-cli 0.5.0 to 0.5.2, no other package moved.
- src/cli/account.rs: `account list` on pimalaya-cli's `AccountListOutput` and `account check`; src/cli/main.rs: `check` hidden, the welcome naming `account check`.
- src/config.rs: `BACKENDS`, `is_built`, `AccountConfig::backends`.
- src/cli/check.rs: the send channel row is `BackendCheck<ErrorCode>`, `ChannelCheck` gone; every row coded through `error::coded`.
- src/json_schema.rs: `neverest-account-list`, `neverest-account-check`, `neverest-check`.
- tests/account.rs (2 tests), `an_account_lists_the_backends_its_endpoints_write`.
- README, MIGRATION, CHANGELOG.

## Capabilities moved

- [sync](../spec/sync.md): *The account group is the family's* added; *A first run meets the family's welcome* amended.
