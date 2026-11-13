---
cairn: log
change: wizard-removed
date: 2026-11-13
---

# The wizard is removed

neverest moves to pimalaya-cli 0.4, through `pimalaya-cli = { path = "../cli" }` in `[patch.crates-io]` (the git pin f976e40 dropped), and removes its configuration wizard, which 0.4 can no longer build: it stood on `wizard::keyring` (`TokenBroker`, `SecretChoice`, `prompt_token`, `prompt_password`, `prompt_store`), `wizard::ortie` (`sign_in`), `wizard::review` and `prompt::items`, all removed by pimalaya-cli's wizards-removed. The owner chose removal on 2026-11-13 over copying about 1000 lines of pimalaya-cli 0.3 into neverest or rewriting the flow on the prompts 0.4 kept.

## What landed

- Cargo.toml: `wizard` left the default set and the feature list; io-pim-discovery left the dependencies and the `native-tls`, `rustls-aws`, `rustls-ring` and `dav` forwards; pimalaya-cli 0.4 in both the dependency and the build dependency. Nothing else used io-pim-discovery, and Cargo.lock lost it.
- Removed src/wizard/ (10 files, 3714 lines) and src/cli/configure.rs (542 lines): `configure` and its alias `cfg`, the `neverest-configure` schema, `offer_configuration` and its no-wizard stub in src/cli/mod.rs.
- src/main.rs: a bare `neverest` prints the help, whatever the configuration; `meet_bare_invocation` is gone.
- src/config.rs: `Config::load_or_wizard(printer, paths)` became `Config::load(paths)`, failing with `No configuration found at <path>, write one by hand: <sample URL>`; its seven callers follow, and `conflict`'s `account` lost the printer it only passed on. `AccountConfig::render`, `RENDER_ORDER`, `ENDPOINT_KEYS`, `set_direct_backend`, `SourceBackendConfig::collection_mut`, `CONFIG_SAMPLE_URL` and the `wizard` flavour of `NO_CONFIG_HINT` went with their only caller, and so did the test `a_generated_config_renders_as_dotted_keys_under_one_header`.
- src/account.rs: `SourceAccount::resolve` removed; `SmtpAccount::resolve` stays for the tests.
- src/cli/main.rs and src/json_schema.rs: `clap::commands::{completion::CompletionCommand, json_schema::{JsonSchemaCommand, JsonSchemaRegistry}, manual::ManualCommand}`, pimalaya-cli 0.4 having dropped the re-exports.
- README (the discovery and wizard bullets, the `wizard` feature note, the first-run section), config.sample.toml, CONTRIBUTING, MIGRATION.md and CHANGELOG (BREAKING under Removed, the Unreleased wizard and io-pim-discovery entries dropped, pimalaya-cli 0.4 bumped).
- cairn/changes/cli-alignment-leftovers: H1 (the local copy of the save layer) is moot.

## Not done

- The first-run welcome cardamum and calendula print (pimalaya-cli `welcome::Welcome`, `is_wanted`, `shipped_sample`) is not adopted: both drive it from pimalaya-config 0.4's `ConfigMissingError`, and neverest is on pimalaya-config 0.3. The 0.4 move (required `sample_url` and `accounts`, `take_account` answering an `AccountError`, `take_default_account` gone) is a change of its own.

## Evidence

With a-refused-line-exits-two: `cargo fmt`; `cargo clippy --all-targets -- -D warnings` clean with default features, `--all-features` and `--no-default-features`; `cargo test` with default features and `--all-features`: 503 unit tests (543 before, the wizard's own tests gone), and every integration test that runs without a server passes (accounts, check, drain, intents, usage). One `--all-features` run failed `a_check_names_an_account_no_configuration_names` at the store's `ensure_collection` unwrap (tests/accounts.rs:303), before any neverest command of the test; it passed alone and in four reruns of the file.

## Capabilities moved

- sync (cairn/spec/sync.md): added "A bare invocation prints the help"; modified "A backend owns its ALPN default", "A collection records the account that syncs it", "The default store root resolves on every supported platform", "Microsoft Graph is a first-class source", "A data command describes what it prints"; removed the 14 wizard requirements and "A bare invocation offers the wizard only on a first run".
