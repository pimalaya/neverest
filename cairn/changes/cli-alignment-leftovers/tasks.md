---
cairn: tasks
change: cli-alignment-leftovers
---

# Tasks

- [ ] F2: replace the bare "Cannot find account" with the family's named failures (src/cli/drain.rs:49, src/cli/sync.rs:179, src/cli/init.rs:38, src/cli/conflict.rs:310, src/cli/check.rs:44), through pimalaya-cli's `resolve_account` if it has shipped
- [ ] F7: add `footer!` from pimalaya-cli to the clap command
- [ ] H3: bump pimalaya-config to 0.2.1 and use `opt_shell_expanded_path`, dropping src/config.rs:982-992
- [ ] A2: once released, drop the git pins for ical @caa1422, io-pimdir @3b4bed8 and io-webdav @62fad89 (Cargo.toml:85, :89, :90)
- [ ] H1: once pimalaya-cli ships the save layer, drop src/cli/configure.rs:100, :253, :279
- [ ] CHANGELOG under [Unreleased]
- [ ] Fold into the spec; log; land
