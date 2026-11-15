---
cairn: change
id: wizard-removed
status: landed
created: 2026-11-13
---

# The wizard is removed

## Why

The owner decided on 2026-11-08 that the Pimalaya CLIs drop their configuration wizards, and pimalaya-cli 0.4 removed them (pimalaya-cli cairn/changes/wizards-removed, landed 2026-11-11): `wizard::{keyring, ortie, review}` and `prompt::items` are gone. neverest's wizard (src/wizard/, `configure`) was built on all four, so it cannot build on pimalaya-cli 0.4. Moving to 0.4 is also what the exit contract needs (a-refused-line-exits-two). The owner chose, on 2026-11-13, to remove neverest's wizard rather than copy pimalaya-cli 0.3's wizard code into neverest (about 1000 lines) or rewrite it on the prompts 0.4 kept.

## What

- Cargo.toml: pimalaya-cli 0.4, from the local tree through `[patch.crates-io]`; the `wizard` feature and the io-pim-discovery dependency are gone, along with its TLS and `rfc6764` forwards.
- src/wizard/ (10 files, 3714 lines) and src/cli/configure.rs (542 lines) are gone: the `configure` command (alias `cfg`), its `neverest-configure` schema, and the offer a bare `neverest` and every command made when no configuration file was found.
- A bare `neverest` prints the help. A command finding no configuration fails naming the path and the sample (`Config::load`, which was `load_or_wizard`).
- What only the wizard used goes too: `AccountConfig::render` with its render order, `set_direct_backend`, `SourceBackendConfig::collection_mut`, `SourceAccount::resolve`, `CONFIG_SAMPLE_URL`, the `offer_configuration` stub.
- Two imports follow pimalaya-cli 0.4's module layout: `clap::commands::{completion, json_schema, manual}`.
- README, config.sample.toml, CONTRIBUTING, MIGRATION and CHANGELOG lose the wizard.

## Not in this change

- The first-run welcome cardamum and calendula print (pimalaya-cli `welcome::Welcome`) is not adopted. It is driven by pimalaya-config 0.4's `ConfigMissingError`, and neverest is still on pimalaya-config 0.3, whose 0.4 changes `take_account` and adds required trait methods. That move is its own change.
