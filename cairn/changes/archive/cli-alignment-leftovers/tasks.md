---
cairn: tasks
change: cli-alignment-leftovers
---

# Tasks

- [x] F2 (2026-11-15, the-command-line-is-the-familys): replace the bare "Cannot find account" with the family's named failures (src/cli/drain.rs:49, src/cli/sync.rs:179, src/cli/init.rs:38, src/cli/conflict.rs:310, src/cli/check.rs:44), through pimalaya-cli's `resolve_account` if it has shipped
- [x] F7 (2026-11-15, the-command-line-is-the-familys): add `footer!` from pimalaya-cli to the clap command
- [x] H3 (2026-11-15, the-family-connection-keys): bump pimalaya-config to 0.2.1 and use `opt_shell_expanded_path`, dropping src/config.rs:982-992
- [x] A2 (2026-11-15, the-released-libraries): once released, drop the git pins for ical @caa1422, io-pimdir @3b4bed8 and io-webdav @62fad89 (Cargo.toml:85, :89, :90)
- [x] H1: moot, src/cli/configure.rs went with the wizard (wizard-removed, 2026-11-13) and pimalaya-cli abandoned wizard-save-layer
- [x] CHANGELOG under [Unreleased]
- [x] Fold into the spec; log; land
